use thiserror::Error;

use crate::{
    ActionHandle, DeviceSession, HostSchema, IdentificationKind, ParameterService,
    ParameterServiceError, ParameterValue, PreflightError, PreflightService, SchemaNumber,
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
    #[error("required parameter '{0}' is not exposed by the HostSchema")]
    MissingParameter(String),
    #[error("required action '{0}' is not exposed by the HostSchema")]
    MissingAction(String),
    #[error("parameter '{parameter}' does not expose enum value '{symbol}'")]
    MissingEnumValue { parameter: String, symbol: String },
    #[error("parameter '{parameter}' enum value '{symbol}' is not a u8 value")]
    InvalidEnumValue { parameter: String, symbol: String },
    #[error("phase-search preflight failed: {0}")]
    PreflightFailed(String),
    #[error("identification preflight failed: {0}")]
    IdentificationPreflightFailed(String),
    #[error("parameter '{0}' does not contain a u8 value")]
    InvalidParameterValue(String),
    #[error("motor must be stopped before starting identification")]
    MotorRunning,
    #[error(transparent)]
    Preflight(#[from] PreflightError),
    #[error(transparent)]
    Parameter(#[from] ParameterServiceError),
    #[error(transparent)]
    Session(#[from] SessionError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentificationStart {
    RequiresEnable,
    Started(ActionHandle),
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

    pub(crate) fn from_shared(
        session: DeviceSession,
        schema: HostSchema,
        parameters: ParameterService,
    ) -> Self {
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
        if let Some(issue) = issues.into_iter().next() {
            return Err(MotorActionError::IdentificationPreflightFailed(issue.reason));
        }

        let mode = self
            .schema
            .parameter_by_key(MOTOR_MODE)
            .ok_or_else(|| MotorActionError::MissingParameter(MOTOR_MODE.to_owned()))?;
        let state = self
            .schema
            .parameter_by_key(MOTOR_STATE)
            .ok_or_else(|| MotorActionError::MissingParameter(MOTOR_STATE.to_owned()))?;

        let ident_mode = enum_u8(mode, IDENT_MODE)?;
        let disabled = enum_u8(state, MOTOR_STATE_DISABLED)?;
        let enabled = enum_u8(state, MOTOR_STATE_ENABLED)?;
        let running = enum_u8(state, MOTOR_STATE_RUN)?;

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
            self.session.action_start(disable.id)?;
            current_state = read_u8(&self.parameters, state)?;
            if current_state != disabled {
                return Err(MotorActionError::InvalidParameterValue(MOTOR_STATE.to_owned()));
            }
        }

        if current_state == disabled {
            self.parameters.write(mode.id, ParameterValue::U8(ident_mode))?;

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

        // Enable is an immediate Action: a successful action_start response means
        // firmware has already executed Motor_Enable(). ACTION_COMPLETE is only
        // emitted for the finite phase-search operation itself.
        self.session.action_start(enable.id)?;

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

fn read_u8(
    parameters: &ParameterService,
    parameter: &crate::ParameterMetadata,
) -> Result<u8, MotorActionError> {
    match parameters.read(parameter.id)? {
        ParameterValue::U8(value) => Ok(value),
        _ => Err(MotorActionError::InvalidParameterValue(parameter.symbol.clone())),
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
