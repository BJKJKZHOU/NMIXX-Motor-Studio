use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::{
    ActionHandle, AxdrStatus, DeviceSession, HostSchema, ParameterService, ParameterServiceError,
    ParameterValue, SchemaNumber, SessionError, SessionEvent,
};

const MOTOR_MODE: &str = "PARAM_MOTOR_MODE";
const PHASE_SEARCH_MODE: &str = "PHASE_SEARCH";
const MOTOR_ENABLE: &str = "ACTION_MOTOR_ENABLE";
const MOTOR_RUN: &str = "ACTION_MOTOR_RUN";
const MOTOR_STOP: &str = "ACTION_MOTOR_STOP";
const MOTOR_DISABLE: &str = "ACTION_MOTOR_DISABLE";
const ACTION_COMPLETION_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum MotorActionError {
    #[error("required parameter '{0}' is not exposed by the HostSchema")]
    MissingParameter(String),
    #[error("required action '{0}' is not exposed by the HostSchema")]
    MissingAction(String),
    #[error("parameter '{parameter}' does not expose enum value '{symbol}'")]
    MissingEnumValue { parameter: String, symbol: String },
    #[error("parameter '{parameter}' enum value '{symbol}' is not a u8 value")]
    InvalidEnumValue { parameter: String, symbol: String },
    #[error("action '{action}' completed with status {status:?}")]
    ActionFailed { action: String, status: AxdrStatus },
    #[error("timed out waiting for action '{0}' to complete")]
    ActionCompletionTimeout(String),
    #[error(transparent)]
    Parameter(#[from] ParameterServiceError),
    #[error(transparent)]
    Session(#[from] SessionError),
}

#[derive(Clone)]
pub struct MotorActionService {
    session: DeviceSession,
    schema: HostSchema,
    parameters: ParameterService,
}

impl MotorActionService {
    pub fn new(session: DeviceSession, schema: HostSchema) -> Self {
        let parameters = ParameterService::new(session.clone(), schema.clone());
        Self { session, schema, parameters }
    }

    /// Start the global motor Enable action.
    pub fn enable(&self) -> Result<ActionHandle, MotorActionError> {
        self.start_action(MOTOR_ENABLE)
    }

    /// Start the global motor Stop action. Firmware returns RUN -> ENABLED.
    pub fn stop(&self) -> Result<ActionHandle, MotorActionError> {
        self.start_action(MOTOR_STOP)
    }

    /// Start the global motor Disable action.
    pub fn disable(&self) -> Result<ActionHandle, MotorActionError> {
        self.start_action(MOTOR_DISABLE)
    }

    /// Start servo phase search as one application-level operation.
    ///
    /// The current AxDr_L firmware exposes phase search through the generic
    /// motor mode + enable + run primitives. Clients must not depend on that
    /// device-side composition, so it is contained here.
    pub fn phase_search_start(&self) -> Result<ActionHandle, MotorActionError> {
        let mode = self
            .schema
            .parameter_by_key(MOTOR_MODE)
            .ok_or_else(|| MotorActionError::MissingParameter(MOTOR_MODE.to_owned()))?;
        let phase_search = enum_u8(mode, PHASE_SEARCH_MODE)?;

        let enable = self
            .schema
            .action_by_key(MOTOR_ENABLE)
            .ok_or_else(|| MotorActionError::MissingAction(MOTOR_ENABLE.to_owned()))?;
        let run = self
            .schema
            .action_by_key(MOTOR_RUN)
            .ok_or_else(|| MotorActionError::MissingAction(MOTOR_RUN.to_owned()))?;

        self.parameters.write(mode.id, ParameterValue::U8(phase_search))?;

        // Subscribe before starting Enable so a fast completion cannot be lost.
        let events = self.session.subscribe()?;
        let enable_handle = self.session.action_start(enable.id)?;
        wait_for_action(&events, enable_handle, MOTOR_ENABLE)?;

        // The returned handle represents the finite phase-search operation.
        Ok(self.session.action_start(run.id)?)
    }

    fn start_action(&self, key: &str) -> Result<ActionHandle, MotorActionError> {
        let action = self
            .schema
            .action_by_key(key)
            .ok_or_else(|| MotorActionError::MissingAction(key.to_owned()))?;
        Ok(self.session.action_start(action.id)?)
    }
}

fn enum_u8(
    parameter: &crate::ParameterMetadata,
    wanted_symbol: &str,
) -> Result<u8, MotorActionError> {
    let index = parameter
        .allowed_symbols
        .iter()
        .position(|symbol| symbol == wanted_symbol)
        .ok_or_else(|| MotorActionError::MissingEnumValue {
            parameter: parameter.symbol.clone(),
            symbol: wanted_symbol.to_owned(),
        })?;

    // Preferred contract: HostSchema supplies numeric allowed values alongside
    // their symbols. The current AxDr exporter only emits allowed_symbols for
    // C enums; those enums are zero-based contiguous and the exported symbol
    // order is the enum ordinal order. Keep that compatibility rule here in
    // the AxDr application adapter rather than leaking a numeric PHASE_SEARCH
    // constant into GUI/CLI clients.
    if let Some(value) = parameter.allowed.get(index) {
        return match value {
            SchemaNumber::Integer(value) => u8::try_from(*value).map_err(|_| MotorActionError::InvalidEnumValue {
                parameter: parameter.symbol.clone(),
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
            _ => Err(MotorActionError::InvalidEnumValue {
                parameter: parameter.symbol.clone(),
                symbol: wanted_symbol.to_owned(),
            }),
        };
    }

    u8::try_from(index).map_err(|_| MotorActionError::InvalidEnumValue {
        parameter: parameter.symbol.clone(),
        symbol: wanted_symbol.to_owned(),
    })
}

fn wait_for_action(
    events: &Receiver<SessionEvent>,
    wanted: ActionHandle,
    action_name: &str,
) -> Result<(), MotorActionError> {
    let deadline = Instant::now() + ACTION_COMPLETION_TIMEOUT;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(MotorActionError::ActionCompletionTimeout(action_name.to_owned()));
        }

        match events.recv_timeout(deadline.saturating_duration_since(now)) {
            Ok(SessionEvent::ActionCompleted { handle, status }) if handle == wanted => {
                if status == AxdrStatus::Ok {
                    return Ok(());
                }
                return Err(MotorActionError::ActionFailed {
                    action: action_name.to_owned(),
                    status,
                });
            }
            Ok(_) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                return Err(MotorActionError::ActionCompletionTimeout(action_name.to_owned()));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(SessionError::Closed.into());
            }
        }
    }
}
