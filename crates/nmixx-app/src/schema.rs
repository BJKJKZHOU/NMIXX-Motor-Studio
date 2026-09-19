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
    #[serde(rename = "label", default)]
    pub name: Option<String>,
    pub id: u16,
    #[serde(rename = "type")]
    pub type_name: String,
    pub access: String,
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActionMetadata {
    pub symbol: String,
    #[serde(rename = "label", default)]
    pub name: Option<String>,
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
        Ok(schema)
    }

    pub fn parameter_by_id(&self, id: u16) -> Option<&ParameterMetadata> {
        self.parameters.iter().find(|parameter| parameter.id == id)
    }

    pub fn parameter_by_key(&self, key: &str) -> Option<&ParameterMetadata> {
        self.parameters.iter().find(|parameter| {
            parameter.symbol == key || parameter.name.as_deref() == Some(key)
        })
    }

    pub fn action_by_id(&self, id: u16) -> Option<&ActionMetadata> {
        self.actions.iter().find(|action| action.id == id)
    }

    pub fn action_by_key(&self, key: &str) -> Option<&ActionMetadata> {
        self.actions
            .iter()
            .find(|action| action.symbol == key || action.name.as_deref() == Some(key))
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
        assert_eq!(parameter.name.as_deref(), Some("Rs"));
        assert_eq!(parameter.parameter_type().unwrap(), ParameterType::F32);
        assert!(parameter.range.as_ref().unwrap().exclusive_min);
        assert_eq!(schema.action_by_key("Enable").unwrap().symbol, "ACTION_MOTOR_ENABLE");
    }
}
