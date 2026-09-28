use thiserror::Error;
use std::sync::Arc;

use crate::parameter_service::ParameterService;
use crate::{
    ActionHandle, DeviceSession, HostSchema, IdentificationKind,
    ParameterServiceError, ParameterValue, PreflightError, PreflightIssue, PreflightService, SchemaError,
    SessionError,
};

const MOTOR_MODE: &str = "PARAM_MOTOR_MODE";
const MOTOR_STATE: &str = "PARAM_MOTOR_STATE";
const IDENT_MODE: &str = "IDENT";
const PHASE_SEARCH_MODE: &str = "PHASE_SEARCH";
const MOTOR_STATE_DISABLED: &str = "DISABLED";
const MOTOR_STATE_ENABLED: &str = "ENABLED";
const MOTOR_STATE_RUN: &str = "RUN";
const MOTOR_ENABLE: &str = "ACTION_MOTOR_ENABLE";
const MOTOR_RUN: &str = "ACTION_MOTOR_RUN";
const MOTOR_STOP: &str = "ACTION_MOTOR_STOP";
const MOTOR_DISABLE: &str = "ACTION_MOTOR_DISABLE";
const IDENT_RS_LS_START: &str = "ACTION_IDENT_RS_LS_START";
const IDENT_FLUX_START: &str = "ACTION_IDENT_FLUX_START";
const IDENT_JB_START: &str = "ACTION_IDENT_JB_START";
const IDENT_APPLY: &str = "ACTION_IDENT_APPLY";

#[derive(Debug, Error)]
pub enum MotorActionError {
    #[error("The pending motor operation was cancelled")]
    Cancelled,
    #[error("required parameter '{0}' is not exposed by the HostSchema")]
    MissingParameter(String),
    #[error("required action '{0}' is not exposed by the HostSchema")]
    MissingAction(String),
    #[error("phase-search preflight failed: {0}")]
    PreflightFailed(String),
    #[error("parameter '{0}' does not contain a u8 value")]
    InvalidParameterValue(String),
    #[error("motor must be stopped before starting this operation")]
    MotorRunning,
    #[error(transparent)]
    Preflight(#[from] PreflightError),
    #[error(transparent)]
    Parameter(#[from] ParameterServiceError),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Schema(#[from] SchemaError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentificationStart {
    Blocked(Vec<PreflightIssue>),
    RequiresEnable,
    Started(ActionHandle),
}

#[derive(Clone)]
pub struct MotorActionService {
    session: DeviceSession,
    schema: HostSchema,
    parameters: ParameterService,
    checkpoint: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
}

impl MotorActionService {
    pub(crate) fn from_shared(
        session: DeviceSession,
        schema: HostSchema,
        parameters: ParameterService,
    ) -> Self {
        Self { session, schema, parameters, checkpoint: None }
    }

    pub(crate) fn with_checkpoint(mut self, checkpoint: Arc<dyn Fn() -> bool + Send + Sync>) -> Self {
        self.checkpoint = Some(checkpoint);
        self
    }
    fn check_dispatch(&self) -> Result<(), MotorActionError> {
        if self.checkpoint.as_ref().is_some_and(|check| !check()) { Err(MotorActionError::Cancelled) } else { Ok(()) }
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

    /// Start one identification operation as an application-level workflow.
    ///
    /// Identification mode is an internal firmware detail. When the motor is
    /// disabled, callers must explicitly authorize enabling before this method
    /// will change mode and energize the drive.
    pub fn identification_start(
        &self,
        kind: IdentificationKind,
        allow_enable: bool,
    ) -> Result<IdentificationStart, MotorActionError> {
        let issues = PreflightService::new(self.parameters.clone()).check_identification(kind)?;
        if !issues.is_empty() {
            return Ok(IdentificationStart::Blocked(issues));
        }

        let mode = self
            .schema
            .parameter_by_key(MOTOR_MODE)
            .ok_or_else(|| MotorActionError::MissingParameter(MOTOR_MODE.to_owned()))?;
        let state = self
            .schema
            .parameter_by_key(MOTOR_STATE)
            .ok_or_else(|| MotorActionError::MissingParameter(MOTOR_STATE.to_owned()))?;

        let ident_mode = mode.enum_u8(IDENT_MODE)?;
        let disabled = state.enum_u8(MOTOR_STATE_DISABLED)?;
        let enabled = state.enum_u8(MOTOR_STATE_ENABLED)?;
        let running = state.enum_u8(MOTOR_STATE_RUN)?;

        let mut current_state = read_u8(&self.parameters, state)?;

        if current_state == running {
            return Err(MotorActionError::MotorRunning);
        }

        if current_state == disabled && !allow_enable {
            return Ok(IdentificationStart::RequiresEnable);
        }

        let enable = self
            .schema
            .action_by_key(MOTOR_ENABLE)
            .ok_or_else(|| MotorActionError::MissingAction(MOTOR_ENABLE.to_owned()))?;
        let disable = self
            .schema
            .action_by_key(MOTOR_DISABLE)
            .ok_or_else(|| MotorActionError::MissingAction(MOTOR_DISABLE.to_owned()))?;

        let current_mode = read_u8(&self.parameters, mode)?;

        if current_state == enabled && current_mode != ident_mode {
            self.check_dispatch()?;
            self.session.action_start(disable.id)?;
            current_state = read_u8(&self.parameters, state)?;
            if current_state != disabled {
                return Err(MotorActionError::InvalidParameterValue(MOTOR_STATE.to_owned()));
            }
        }

        if current_state == disabled {
            self.check_dispatch()?;
            self.parameters.write(mode.id, ParameterValue::U8(ident_mode))?;

            self.check_dispatch()?;
            self.session.action_start(enable.id)?;

            current_state = read_u8(&self.parameters, state)?;
            if current_state != enabled {
                return Err(MotorActionError::InvalidParameterValue(MOTOR_STATE.to_owned()));
            }
        }

        let action_key = match kind {
            IdentificationKind::RsLs => IDENT_RS_LS_START,
            IdentificationKind::Flux => IDENT_FLUX_START,
            IdentificationKind::Jb => IDENT_JB_START,
        };
        let action = self
            .schema
            .action_by_key(action_key)
            .ok_or_else(|| MotorActionError::MissingAction(action_key.to_owned()))?;

        self.check_dispatch()?;
        Ok(IdentificationStart::Started(self.session.action_start(action.id)?))
    }

    /// Apply the latest valid identification result.
    ///
    /// Firmware processes IDENT_APPLY synchronously before returning the
    /// Action response, so callers must not wait for ACTION_COMPLETE.
    pub fn identification_apply(&self) -> Result<ActionHandle, MotorActionError> {
        self.start_action(IDENT_APPLY)
    }

    /// Start servo phase search as one application-level operation.
    ///
    /// The current AxDr_L firmware exposes phase search through the generic
    /// motor mode + enable + run primitives. Clients must not depend on that
    /// device-side composition, so it is contained here.
    pub fn phase_search_start(&self) -> Result<ActionHandle, MotorActionError> {
        let issues = PreflightService::new(self.parameters.clone()).check_phase_search()?;
        if let Some(issue) = issues.into_iter().next() {
            return Err(MotorActionError::PreflightFailed(issue.reason));
        }

        let mode = self
            .schema
            .parameter_by_key(MOTOR_MODE)
            .ok_or_else(|| MotorActionError::MissingParameter(MOTOR_MODE.to_owned()))?;
        let phase_search = mode.enum_u8(PHASE_SEARCH_MODE)?;
        let state = self
            .schema
            .parameter_by_key(MOTOR_STATE)
            .ok_or_else(|| MotorActionError::MissingParameter(MOTOR_STATE.to_owned()))?;
        let disabled = state.enum_u8(MOTOR_STATE_DISABLED)?;
        let enabled = state.enum_u8(MOTOR_STATE_ENABLED)?;
        let running = state.enum_u8(MOTOR_STATE_RUN)?;

        let enable = self
            .schema
            .action_by_key(MOTOR_ENABLE)
            .ok_or_else(|| MotorActionError::MissingAction(MOTOR_ENABLE.to_owned()))?;
        let disable = self
            .schema
            .action_by_key(MOTOR_DISABLE)
            .ok_or_else(|| MotorActionError::MissingAction(MOTOR_DISABLE.to_owned()))?;
        let run = self
            .schema
            .action_by_key(MOTOR_RUN)
            .ok_or_else(|| MotorActionError::MissingAction(MOTOR_RUN.to_owned()))?;

        let mut current_state = read_u8(&self.parameters, state)?;
        if current_state == running {
            return Err(MotorActionError::MotorRunning);
        }

        let current_mode = read_u8(&self.parameters, mode)?;

        // Motor mode is DISABLED-only in the current firmware. Keep that
        // device-side detail inside the semantic operation: if the drive is
        // already ENABLED in another mode, disable it before selecting
        // PHASE_SEARCH, then restore ENABLED before the finite Run.
        if current_state == enabled && current_mode != phase_search {
            self.check_dispatch()?;
            self.session.action_start(disable.id)?;
            current_state = read_u8(&self.parameters, state)?;
            if current_state != disabled {
                return Err(MotorActionError::InvalidParameterValue(MOTOR_STATE.to_owned()));
            }
        }

        if current_state == disabled {
            self.check_dispatch()?;
            self.parameters.write(mode.id, ParameterValue::U8(phase_search))?;

            // Enable is an immediate Action: a successful action_start response
            // means firmware has already executed Motor_Enable().
            self.check_dispatch()?;
            self.session.action_start(enable.id)?;

            current_state = read_u8(&self.parameters, state)?;
            if current_state != enabled {
                return Err(MotorActionError::InvalidParameterValue(MOTOR_STATE.to_owned()));
            }
        }

        // If the drive was already ENABLED in PHASE_SEARCH mode, no redundant
        // Disable/Enable cycle is needed. The returned handle represents the
        // finite Run/phase-search operation.
        self.check_dispatch()?;
        Ok(self.session.action_start(run.id)?)
    }

    fn start_action(&self, key: &str) -> Result<ActionHandle, MotorActionError> {
        let action = self
            .schema
            .action_by_key(key)
            .ok_or_else(|| MotorActionError::MissingAction(key.to_owned()))?;
        self.check_dispatch()?;
        Ok(self.session.action_start(action.id)?)
    }
}

fn read_u8(
    parameters: &ParameterService,
    parameter: &crate::ParameterMetadata,
) -> Result<u8, MotorActionError> {
    match parameters.read(parameter.id)? {
        ParameterValue::U8(value) => Ok(value),
        _ => Err(MotorActionError::InvalidParameterValue(parameter.symbol.clone())),
    }
}

