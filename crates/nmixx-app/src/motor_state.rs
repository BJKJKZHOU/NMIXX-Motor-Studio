use std::fmt;

use crate::ParameterMetadata;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotorState {
    Disabled,
    Enabled,
    Run,
}

impl MotorState {
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Disabled => "DISABLED",
            Self::Enabled => "ENABLED",
            Self::Run => "RUN",
        }
    }

    pub(crate) fn from_parameter(parameter: &ParameterMetadata, raw: u8) -> Result<Self, String> {
        let symbol = parameter
            .enum_symbol_u8(raw)
            .ok_or_else(|| format!("unsupported motor state value {raw}"))?;
        match symbol {
            "DISABLED" => Ok(Self::Disabled),
            "ENABLED" => Ok(Self::Enabled),
            "RUN" => Ok(Self::Run),
            other => Err(format!("unsupported motor state symbol {other}")),
        }
    }
}

impl fmt::Display for MotorState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.symbol())
    }
}
