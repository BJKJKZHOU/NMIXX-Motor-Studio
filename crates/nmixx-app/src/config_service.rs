use thiserror::Error;

use crate::{
    ActionHandle, DeviceSession, HostSchema, ParameterService, ParameterServiceError,
    ParameterValue, SessionError,
};

const MOTOR_STATE: &str = "PARAM_MOTOR_STATE";
const MOTOR_STATE_DISABLED: &str = "DISABLED";
const PARAMETER_SAVE: &str = "ACTION_PARAMETER_SAVE";

#[derive(Debug, Error)]
pub enum ConfigServiceError {
    #[error("required parameter '{0}' is not exposed by the HostSchema")]
    MissingParameter(String),
    #[error("required action '{0}' is not exposed by the HostSchema")]
    MissingAction(String),
    #[error("parameter '{parameter}' does not expose enum value '{symbol}'")]
    MissingEnumValue { parameter: String, symbol: String },
    #[error("parameter '{parameter}' enum value '{symbol}' is not a u8 value")]
    InvalidEnumValue { parameter: String, symbol: String },
    #[error("persistent configuration can only be saved while the motor is DISABLED")]
    MotorNotDisabled,
    #[error(transparent)]
    Parameter(#[from] ParameterServiceError),
    #[error(transparent)]
    Session(#[from] SessionError),
}

#[derive(Clone)]
pub struct ConfigService {
    session: DeviceSession,
    schema: HostSchema,
    parameters: ParameterService,
}

impl ConfigService {
    pub fn new(session: DeviceSession, schema: HostSchema) -> Self {
        let parameters = ParameterService::new(session.clone(), schema.clone());
        Self {
            session,
            schema,
            parameters,
        }
    }

    pub(crate) fn from_shared(
        session: DeviceSession,
        schema: HostSchema,
        parameters: ParameterService,
    ) -> Self {
        Self {
            session,
            schema,
            parameters,
        }
    }

    pub fn save_available(&self) -> bool {
        self.schema.action_by_key(PARAMETER_SAVE).is_some()
    }

    pub fn save(&self) -> Result<ActionHandle, ConfigServiceError> {
        let state = self
            .schema
            .parameter_by_key(MOTOR_STATE)
            .ok_or_else(|| ConfigServiceError::MissingParameter(MOTOR_STATE.to_owned()))?;
        let disabled = enum_u8(state, MOTOR_STATE_DISABLED)?;

        match self.parameters.read(state.id)? {
            ParameterValue::U8(value) if value == disabled => {}
            ParameterValue::U8(_) => return Err(ConfigServiceError::MotorNotDisabled),
            _ => {
                return Err(ConfigServiceError::InvalidEnumValue {
                    parameter: MOTOR_STATE.to_owned(),
                    symbol: MOTOR_STATE_DISABLED.to_owned(),
                });
            }
        }

        let action = self
            .schema
            .action_by_key(PARAMETER_SAVE)
            .ok_or_else(|| ConfigServiceError::MissingAction(PARAMETER_SAVE.to_owned()))?;
        Ok(self.session.action_start(action.id)?)
    }
}

fn enum_u8(
    parameter: &crate::ParameterMetadata,
    wanted_symbol: &str,
) -> Result<u8, ConfigServiceError> {
    let index = parameter
        .allowed_symbols
        .iter()
        .position(|symbol| symbol == wanted_symbol)
        .ok_or_else(|| ConfigServiceError::MissingEnumValue {
            parameter: parameter.symbol.clone(),
            symbol: wanted_symbol.to_owned(),
        })?;

    if let Some(value) = parameter.allowed.get(index) {
        return match value {
            crate::SchemaNumber::Integer(value) => {
                u8::try_from(*value).map_err(|_| ConfigServiceError::InvalidEnumValue {
                    parameter: parameter.symbol.clone(),
                    symbol: wanted_symbol.to_owned(),
                })
            }
            crate::SchemaNumber::Float(value)
                if value.is_finite()
                    && value.fract() == 0.0
                    && *value >= 0.0
                    && *value <= u8::MAX as f64 =>
            {
                Ok(*value as u8)
            }
            _ => Err(ConfigServiceError::InvalidEnumValue {
                parameter: parameter.symbol.clone(),
                symbol: wanted_symbol.to_owned(),
            }),
        };
    }

    u8::try_from(index).map_err(|_| ConfigServiceError::InvalidEnumValue {
        parameter: parameter.symbol.clone(),
        symbol: wanted_symbol.to_owned(),
    })
}
