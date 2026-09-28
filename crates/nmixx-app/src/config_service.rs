use thiserror::Error;

use crate::parameter_service::ParameterService;
use crate::{
    ActionHandle, DeviceSession, HostSchema, ParameterServiceError,
    ParameterValue, SchemaError, SessionError,
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
    #[error("persistent configuration can only be saved while the motor is DISABLED")]
    MotorNotDisabled,
    #[error(transparent)]
    Parameter(#[from] ParameterServiceError),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Schema(#[from] SchemaError),
}

#[derive(Clone)]
pub struct ConfigService {
    session: DeviceSession,
    schema: HostSchema,
    parameters: ParameterService,
}

impl ConfigService {
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
        let disabled = state.enum_u8(MOTOR_STATE_DISABLED)?;

        match self.parameters.read(state.id)? {
            ParameterValue::U8(value) if value == disabled => {}
            ParameterValue::U8(_) => return Err(ConfigServiceError::MotorNotDisabled),
            _ => return Err(ConfigServiceError::MotorNotDisabled),
        }

        let action = self
            .schema
            .action_by_key(PARAMETER_SAVE)
            .ok_or_else(|| ConfigServiceError::MissingAction(PARAMETER_SAVE.to_owned()))?;
        Ok(self.session.action_start(action.id)?)
    }
}

