use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use crate::ParameterType;

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("failed to read Host schema: {0}")]
    Io(String),
    #[error("failed to parse Host schema TOML: {0}")]
    Parse(String),
    #[error("unsupported Host schema version {0}")]
    UnsupportedVersion(u32),
    #[error("unknown parameter type '{0}'")]
    UnknownParameterType(String),
    #[error("parameter '{parameter}' does not expose enum value '{symbol}'")]
    MissingEnumValue { parameter: String, symbol: String },
    #[error("parameter '{parameter}' enum value '{symbol}' is not a u8 value")]
    InvalidEnumValue { parameter: String, symbol: String },
    #[error("parameter '{parameter}' has invalid readback target '{target}': {reason}")]
    InvalidReadback { parameter: String, target: String, reason: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct HostSchema {
    pub schema_version: u32,
    pub protocol: String,
    pub source: SchemaSource,
    #[serde(default)]
    pub parameters: Vec<ParameterMetadata>,
    #[serde(default)]
    pub actions: Vec<ActionMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SchemaSource {
    pub repository: String,
    pub git_sha: String,
    pub parameter_schema: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParameterMetadata {
    pub symbol: String,
    pub label: String,
    pub id: u16,
    #[serde(rename = "type")]
    pub type_name: String,
    pub access: String,
    #[serde(default)]
    pub persistent: bool,
    #[serde(default)]
    pub unit: Option<String>,
    pub description: String,
    #[serde(default)]
    pub write_state: Option<String>,
    #[serde(default)]
    pub range: Option<RangeMetadata>,
    #[serde(default)]
    pub allowed: Vec<SchemaNumber>,
    #[serde(default)]
    pub allowed_symbols: Vec<String>,
    #[serde(default)]
    pub readback: Vec<String>,
    #[serde(default)]
    pub plot_scale: Option<f64>,
}

impl ParameterMetadata {
    pub fn parameter_type(&self) -> Result<ParameterType, SchemaError> {
        Ok(match self.type_name.as_str() {
            "u8" => ParameterType::U8,
            "i8" => ParameterType::I8,
            "f32" => ParameterType::F32,
            "i32" => ParameterType::I32,
            "u32" => ParameterType::U32,
            "position" => ParameterType::Position,
            other => return Err(SchemaError::UnknownParameterType(other.to_owned())),
        })
    }

    /// Resolve one HostSchema enum symbol to its u8 wire value. Prefer explicit
    /// `allowed` values; when the exporter provides only `allowed_symbols`,
    /// their order is the zero-based enum ordinal contract.
    pub fn enum_u8(&self, wanted_symbol: &str) -> Result<u8, SchemaError> {
        let index = self
            .allowed_symbols
            .iter()
            .position(|symbol| symbol == wanted_symbol)
            .ok_or_else(|| SchemaError::MissingEnumValue {
                parameter: self.symbol.clone(),
                symbol: wanted_symbol.to_owned(),
            })?;

        if let Some(value) = self.allowed.get(index) {
            return match value {
                SchemaNumber::Integer(value) => u8::try_from(*value).map_err(|_| SchemaError::InvalidEnumValue {
                    parameter: self.symbol.clone(),
                    symbol: wanted_symbol.to_owned(),
                }),
                SchemaNumber::Float(value)
                    if value.is_finite()
                        && value.fract() == 0.0
                        && *value >= 0.0
                        && *value <= u8::MAX as f64 =>
                {
                    Ok(*value as u8)
                }
                _ => Err(SchemaError::InvalidEnumValue {
                    parameter: self.symbol.clone(),
                    symbol: wanted_symbol.to_owned(),
                }),
            };
        }

        u8::try_from(index).map_err(|_| SchemaError::InvalidEnumValue {
            parameter: self.symbol.clone(),
            symbol: wanted_symbol.to_owned(),
        })
    }

    pub fn enum_symbol_u8(&self, raw: u8) -> Option<&str> {
        self.allowed_symbols.iter().enumerate().find_map(|(index, symbol)| {
            self.enum_u8(symbol).ok().filter(|value| *value == raw).map(|_| symbol.as_str())
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActionMetadata {
    pub symbol: String,
    pub label: String,
    pub id: u16,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RangeMetadata {
    #[serde(default)]
    pub min: Option<SchemaNumber>,
    #[serde(default)]
    pub max: Option<SchemaNumber>,
    #[serde(default)]
    pub exclusive_min: bool,
    #[serde(default)]
    pub exclusive_max: bool,
    #[serde(default)]
    pub max_symbol: Option<String>,
    #[serde(default)]
    pub min_binding: Option<String>,
    #[serde(default)]
    pub max_binding: Option<String>,
    #[serde(default)]
    pub max_bindings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SchemaNumber {
    Integer(i64),
    Float(f64),
}

impl HostSchema {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, SchemaError> {
        let text = fs::read_to_string(path).map_err(|error| SchemaError::Io(error.to_string()))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, SchemaError> {
        let schema: Self = toml::from_str(text).map_err(|error| SchemaError::Parse(error.to_string()))?;
        if schema.schema_version != 1 {
            return Err(SchemaError::UnsupportedVersion(schema.schema_version));
        }
        schema.validate_readback()?;
        Ok(schema)
    }

    fn validate_readback(&self) -> Result<(), SchemaError> {
        for parameter in &self.parameters {
            if !parameter.readback.is_empty() && !parameter.access.contains('w') {
                return Err(SchemaError::InvalidReadback {
                    parameter: parameter.symbol.clone(),
                    target: parameter.symbol.clone(),
                    reason: "readback metadata requires a writable source Parameter".to_owned(),
                });
            }
            let mut seen = HashSet::new();
            for target in &parameter.readback {
                if target == &parameter.symbol {
                    return Err(SchemaError::InvalidReadback {
                        parameter: parameter.symbol.clone(),
                        target: target.clone(),
                        reason: "source Parameter is read back implicitly".to_owned(),
                    });
                }
                if !seen.insert(target) {
                    return Err(SchemaError::InvalidReadback {
                        parameter: parameter.symbol.clone(),
                        target: target.clone(),
                        reason: "duplicate target".to_owned(),
                    });
                }
                let Some(metadata) = self.parameter_by_symbol(target) else {
                    return Err(SchemaError::InvalidReadback {
                        parameter: parameter.symbol.clone(),
                        target: target.clone(),
                        reason: "target is not present in HostSchema".to_owned(),
                    });
                };
                if !metadata.access.contains('r') {
                    return Err(SchemaError::InvalidReadback {
                        parameter: parameter.symbol.clone(),
                        target: target.clone(),
                        reason: "target is not readable".to_owned(),
                    });
                }
            }
        }
        Ok(())
    }

    pub fn parameter_by_id(&self, id: u16) -> Option<&ParameterMetadata> {
        self.parameters.iter().find(|parameter| parameter.id == id)
    }

    pub fn parameter_by_symbol(&self, symbol: &str) -> Option<&ParameterMetadata> {
        self.parameters.iter().find(|parameter| parameter.symbol == symbol)
    }

    pub fn parameter_by_key(&self, key: &str) -> Option<&ParameterMetadata> {
        self.parameters.iter().find(|parameter| {
            parameter.symbol == key || parameter.label == key
        })
    }

    pub fn action_by_id(&self, id: u16) -> Option<&ActionMetadata> {
        self.actions.iter().find(|action| action.id == id)
    }

    pub fn action_by_key(&self, key: &str) -> Option<&ActionMetadata> {
        self.actions
            .iter()
            .find(|action| action.symbol == key || action.label == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_host_schema() {
        let schema = HostSchema::parse(
            r#"
schema_version = 1
protocol = "axdr-canfd-v1"

[source]
repository = "AxDr_L_Motor"
git_sha = "abc123"
parameter_schema = 1

[[parameters]]
symbol = "PARAM_MOTOR_RS"
label = "Rs"
id = 272
type = "f32"
access = "rw"
unit = "ohm"
description = "Motor phase resistance"

[parameters.range]
min = 0.0
exclusive_min = true

[[actions]]
symbol = "ACTION_MOTOR_ENABLE"
label = "Enable"
id = 4097
description = "Enable motor"
"#,
        )
        .unwrap();

        let parameter = schema.parameter_by_key("Rs").unwrap();
        assert_eq!(parameter.id, 272);
        assert_eq!(parameter.label, "Rs");
        assert_eq!(parameter.parameter_type().unwrap(), ParameterType::F32);
        assert!(parameter.range.as_ref().unwrap().exclusive_min);
        assert_eq!(schema.action_by_key("Enable").unwrap().symbol, "ACTION_MOTOR_ENABLE");
    }
    #[test]
    fn parses_and_validates_parameter_readback_metadata() {
        let schema = HostSchema::parse(r#"
schema_version = 1
protocol = "axdr-canfd-v1"

[source]
repository = "fixture"
git_sha = "abc"
parameter_schema = 1

[[parameters]]
symbol = "PARAM_SOURCE"
label = "Source"
id = 1
type = "f32"
access = "rw"
description = "source"
readback = ["PARAM_DERIVED"]

[[parameters]]
symbol = "PARAM_DERIVED"
label = "Derived"
id = 2
type = "f32"
access = "ro"
description = "derived"
"#).unwrap();

        assert_eq!(
            schema.parameter_by_symbol("PARAM_SOURCE").unwrap().readback,
            vec!["PARAM_DERIVED".to_owned()]
        );
    }

    #[test]
    fn rejects_unknown_parameter_readback_target() {
        let error = HostSchema::parse(r#"
schema_version = 1
protocol = "axdr-canfd-v1"

[source]
repository = "fixture"
git_sha = "abc"
parameter_schema = 1

[[parameters]]
symbol = "PARAM_SOURCE"
label = "Source"
id = 1
type = "f32"
access = "rw"
description = "source"
readback = ["PARAM_MISSING"]
"#).unwrap_err();

        assert!(matches!(error, SchemaError::InvalidReadback { .. }));
    }

    #[test]
    fn parses_persistent_metadata_and_defaults_to_false() {
        let schema = HostSchema::parse(r#"
schema_version = 1
protocol = "axdr-canfd-v1"

[source]
repository = "fixture"
git_sha = "abc"
parameter_schema = 1

[[parameters]]
symbol = "PARAM_SAVED"
label = "Saved"
id = 1
type = "f32"
access = "rw"
persistent = true
description = "saved"

[[parameters]]
symbol = "PARAM_RAM_ONLY"
label = "RAM only"
id = 2
type = "f32"
access = "rw"
description = "ram"
"#).unwrap();

        assert!(schema.parameter_by_symbol("PARAM_SAVED").unwrap().persistent);
        assert!(!schema.parameter_by_symbol("PARAM_RAM_ONLY").unwrap().persistent);
    }

    #[test]
    fn enum_u8_prefers_explicit_allowed_values_and_falls_back_to_ordinal() {
        let explicit = ParameterMetadata {
            symbol: "STATE".into(), label: "State".into(), id: 1, type_name: "u8".into(),
            access: "ro".into(), persistent: false, unit: None, description: String::new(), write_state: None,
            range: None, allowed: vec![SchemaNumber::Integer(4), SchemaNumber::Integer(9)],
            allowed_symbols: vec!["IDLE".into(), "RUN".into()], readback: Vec::new(), plot_scale: None,
        };
        assert_eq!(explicit.enum_u8("RUN").unwrap(), 9);
        assert_eq!(explicit.enum_symbol_u8(4), Some("IDLE"));

        let ordinal = ParameterMetadata { allowed: Vec::new(), ..explicit };
        assert_eq!(ordinal.enum_u8("RUN").unwrap(), 1);
        assert_eq!(ordinal.enum_symbol_u8(0), Some("IDLE"));
    }

}
