use thiserror::Error;
use std::sync::Arc;

use crate::parameter_service::ParameterService;
use crate::preflight::PreflightService;
use crate::{
    ActionHandle, DeviceSession, HostSchema, IdentificationKind,
    ParameterServiceError, ParameterValue, PreflightError, PreflightIssue, SchemaError,
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


#[cfg(test)]
mod first_motor_wire_tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Arc, Mutex};
    use nmixx_core::protocol::{
        AxdrStatus, ParameterType, MSG_PARAMETER, MSG_RESPONSE, NODE_ID_DEFAULT,
        PARAM_READ, PARAM_WRITE, can_id,
    };
    use nmixx_core::transport::{FrameTransport, TransportError};
    use nmixx_core::wire::CanFdFrame;

    const MOTOR_MODE_ID: u16 = 0x0701;
    const MOTOR_STATE_ID: u16 = 0x0710;
    const ENABLE_ID: u16 = 0x1001;
    const DISABLE_ID: u16 = 0x1004;
    const RUN_ID: u16 = 0x1002;
    const RSLS_ID: u16 = 0x1201;
    const FLUX_ID: u16 = 0x1202;
    const JB_ID: u16 = 0x1203;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Sent {
        Write(u16, u8),
        Action(u16),
    }
    struct Model {
        parameters: HashMap<u16, ParameterValue>,
        sent: Vec<Sent>,
    }
    struct Device {
        model: Arc<Mutex<Model>>,
        incoming: VecDeque<CanFdFrame>,
    }
    impl FrameTransport for Device {
        fn send(&mut self, frame: &CanFdFrame) -> Result<(), TransportError> {
            let p = frame.data();
            let txn = p[0];
            let op = p[1];
            let id = u16::from_le_bytes([p[2], p[3]]);
            let mut model = self.model.lock().unwrap();
            let mut response = vec![txn, MSG_PARAMETER, op, AxdrStatus::Ok as u8, p[2], p[3]];
            if op == PARAM_READ {
                let value = model.parameters.get(&id).copied().unwrap();
                match value {
                    ParameterValue::U8(value) => response.extend([ParameterType::U8 as u8, value]),
                    ParameterValue::U32(value) => {
                        response.push(ParameterType::U32 as u8);
                        response.extend(value.to_le_bytes());
                    }
                    ParameterValue::F32(value) => {
                        response.push(ParameterType::F32 as u8);
                        response.extend(value.to_le_bytes());
                    }
                    _ => panic!("unsupported fixture Parameter"),
                }
            } else if op == PARAM_WRITE {
                let kind = p[4];
                if kind == ParameterType::Action as u8 {
                    model.sent.push(Sent::Action(id));
                    match id {
                        ENABLE_ID => { model.parameters.insert(MOTOR_STATE_ID, ParameterValue::U8(1)); }
                        DISABLE_ID => { model.parameters.insert(MOTOR_STATE_ID, ParameterValue::U8(0)); }
                        RUN_ID | RSLS_ID | FLUX_ID | JB_ID => {
                            model.parameters.insert(MOTOR_STATE_ID, ParameterValue::U8(2));
                        }
                        _ => {}
                    }
                } else if kind == ParameterType::U8 as u8 {
                    model.sent.push(Sent::Write(id, p[5]));
                    model.parameters.insert(id, ParameterValue::U8(p[5]));
                } else {
                    panic!("unsupported fixture Write");
                }
            } else {
                panic!("unexpected protocol operation");
            }
            self.incoming.push_back(
                CanFdFrame::new(can_id(MSG_RESPONSE, NODE_ID_DEFAULT), &response).unwrap()
            );
            Ok(())
        }
        fn receive(&mut self, _timeout: std::time::Duration) -> Result<CanFdFrame, TransportError> {
            self.incoming.pop_front().ok_or(TransportError::Timeout)
        }
    }

    fn system() -> (MotorActionService, Arc<Mutex<Model>>) {
        let entries: &[(&str, u16, &str)] = &[
            ("PARAM_EVENT_ERROR", 0x0803, "u32"),
            ("PARAM_EVENT_TRIP", 0x0804, "u32"),
            ("PARAM_LIMIT_I_MAX", 0x0201, "f32"),
            ("PARAM_LIMIT_WM_MAX", 0x0202, "f32"),
            ("PARAM_MOTOR_PP", 0x0101, "u8"),
            ("PARAM_MOTOR_RS", 0x0110, "f32"),
            ("PARAM_MOTOR_LD", 0x0111, "f32"),
            ("PARAM_MOTOR_LQ", 0x0112, "f32"),
            ("PARAM_MOTOR_FLUX", 0x0113, "f32"),
            ("PARAM_MOTOR_ALIGN_CURRENT", 0x0117, "f32"),
            ("PARAM_MOTOR_IF_CURRENT", 0x0118, "f32"),
            ("PARAM_MOTOR_RL_I_PEAK", 0x0119, "f32"),
            ("PARAM_IDENT_JB_EXCITE_RATIO", 0x0923, "f32"),
            ("PARAM_IDENT_JB_EXCITE_HZ", 0x0924, "f32"),
            ("PARAM_ENCODER_READY", 0x0302, "u8"),
            ("PARAM_ENCODER_VALID", 0x0303, "u8"),
            ("PARAM_ENCODER_FAULT", 0x0304, "u8"),
            ("PARAM_PHASE_I_SEARCH", 0x0401, "f32"),
            (MOTOR_MODE, MOTOR_MODE_ID, "u8"),
            (MOTOR_STATE, MOTOR_STATE_ID, "u8"),
        ];
        let actions: &[(&str, u16)] = &[
            (MOTOR_ENABLE, ENABLE_ID), (MOTOR_DISABLE, DISABLE_ID),
            (MOTOR_RUN, RUN_ID), (IDENT_RS_LS_START, RSLS_ID),
            (IDENT_FLUX_START, FLUX_ID), (IDENT_JB_START, JB_ID),
        ];
        let mut schema = String::from(
            "schema_version = 1\nprotocol = \"axdr-canfd-v1\"\n[source]\nrepository = \"fixture\"\ngit_sha = \"abc\"\nparameter_schema = 1\n"
        );
        for (symbol, id, typ) in entries {
            schema.push_str(&format!(
                "\n[[parameters]]\nsymbol = \"{symbol}\"\nlabel = \"{symbol}\"\nid = {id}\ntype = \"{typ}\"\naccess = \"{}\"\ndescription = \"test\"\n",
                if *symbol == MOTOR_MODE { "rw" } else { "ro" }
            ));
            if *symbol == MOTOR_MODE {
                schema.push_str("allowed_symbols = [\"TORQUE\", \"IDENT\", \"PHASE_SEARCH\"]\nwrite_state = \"disabled\"\n");
            }
            if *symbol == MOTOR_STATE {
                schema.push_str("allowed_symbols = [\"DISABLED\", \"ENABLED\", \"RUN\"]\n");
            }
        }
        for (symbol, id) in actions {
            schema.push_str(&format!(
                "\n[[actions]]\nsymbol = \"{symbol}\"\nlabel = \"{symbol}\"\nid = {id}\ndescription = \"test\"\n"
            ));
        }
        let mut parameters = HashMap::from([
            (0x0803, ParameterValue::U32(0)), (0x0804, ParameterValue::U32(0)),
            (0x0201, ParameterValue::F32(2.0)), (0x0202, ParameterValue::F32(40.0)),
            (0x0101, ParameterValue::U8(7)),
            (0x0110, ParameterValue::F32(0.38)),
            (0x0111, ParameterValue::F32(0.00015)),
            (0x0112, ParameterValue::F32(0.00015)),
            (0x0113, ParameterValue::F32(0.007)),
            (0x0117, ParameterValue::F32(0.5)), (0x0118, ParameterValue::F32(0.5)),
            (0x0119, ParameterValue::F32(2.0)),
            (0x0923, ParameterValue::F32(0.2)), (0x0924, ParameterValue::F32(3.0)),
            (0x0302, ParameterValue::U8(1)), (0x0303, ParameterValue::U8(1)),
            (0x0304, ParameterValue::U8(0)), (0x0401, ParameterValue::F32(0.4)),
            (MOTOR_MODE_ID, ParameterValue::U8(0)),
            (MOTOR_STATE_ID, ParameterValue::U8(0)),
        ]);
        // Always emulate a fresh powered-off drive, independent of the
        // runtime status in previous tests.
        parameters.insert(MOTOR_STATE_ID, ParameterValue::U8(0));
        let model = Arc::new(Mutex::new(Model { parameters, sent: Vec::new() }));
        let schema = HostSchema::parse(&schema).unwrap();
        let session = DeviceSession::spawn(Box::new(Device {
            model: model.clone(), incoming: VecDeque::new(),
        }));
        let parameters = ParameterService::new(session.clone(), schema.clone());
        (MotorActionService::from_shared(session, schema, parameters), model)
    }

    #[test]
    fn new_motor_commands_follow_real_identification_and_phase_search_semantics() {
        let (service, model) = system();
        assert!(matches!(
            service.identification_start(IdentificationKind::RsLs, false).unwrap(),
            IdentificationStart::Blocked(issues) if issues.iter().any(|i| i.reason.contains("injection peak"))
        ));
        assert!(model.lock().unwrap().sent.is_empty());

        model.lock().unwrap().parameters.insert(0x0119, ParameterValue::F32(0.5));
        assert_eq!(
            service.identification_start(IdentificationKind::RsLs, false).unwrap(),
            IdentificationStart::RequiresEnable
        );
        assert!(model.lock().unwrap().sent.is_empty());

        assert!(matches!(
            service.identification_start(IdentificationKind::RsLs, true).unwrap(),
            IdentificationStart::Started(_)
        ));
        assert_eq!(model.lock().unwrap().sent, vec![
            Sent::Write(MOTOR_MODE_ID, 1),
            Sent::Action(ENABLE_ID),
            Sent::Action(RSLS_ID),
        ]);

        // Scripted finite completion: firmware returns to ENABLED.
        model.lock().unwrap().parameters.insert(MOTOR_STATE_ID, ParameterValue::U8(1));
        assert!(matches!(
            service.identification_start(IdentificationKind::Flux, false).unwrap(),
            IdentificationStart::Started(_)
        ));
        model.lock().unwrap().parameters.insert(MOTOR_STATE_ID, ParameterValue::U8(1));
        assert!(matches!(
            service.identification_start(IdentificationKind::Jb, false).unwrap(),
            IdentificationStart::Started(_)
        ));
        assert_eq!(model.lock().unwrap().sent, vec![
            Sent::Write(MOTOR_MODE_ID, 1),
            Sent::Action(ENABLE_ID),
            Sent::Action(RSLS_ID),
            Sent::Action(FLUX_ID),
            Sent::Action(JB_ID),
        ]);

        model.lock().unwrap().parameters.insert(MOTOR_STATE_ID, ParameterValue::U8(1));
        assert!(service.phase_search_start().is_ok());
        assert_eq!(model.lock().unwrap().sent, vec![
            Sent::Write(MOTOR_MODE_ID, 1),
            Sent::Action(ENABLE_ID),
            Sent::Action(RSLS_ID),
            Sent::Action(FLUX_ID),
            Sent::Action(JB_ID),
            Sent::Action(DISABLE_ID),
            Sent::Write(MOTOR_MODE_ID, 2),
            Sent::Action(ENABLE_ID),
            Sent::Action(RUN_ID),
        ]);
    }
}
