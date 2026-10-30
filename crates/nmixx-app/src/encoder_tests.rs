//! Current firmware contract: task Actions own Enable/Run and final Disable.
use super::*;
use std::collections::{HashMap, VecDeque};
use nmixx_core::protocol::{can_id, ParameterType, PositionValue, EVENT_ACTION_COMPLETE,
    MSG_EVENT, MSG_PARAMETER, MSG_RESPONSE, NODE_ID_DEFAULT, PARAM_READ, PARAM_WRITE};
use nmixx_core::transport::{FrameTransport, TransportError};
use nmixx_core::wire::CanFdFrame;

struct Device {
    values: HashMap<u16, ParameterValue>,
    incoming: VecDeque<CanFdFrame>,
    writes: Vec<u16>,
    reject_start: bool,
}
#[derive(Clone)]
struct Wire { schema: HostSchema, device: Arc<Mutex<Device>> }
impl Wire {
    fn id(&self, symbol: &str) -> u16 {
        self.schema.parameter_by_key(symbol).map(|p| p.id)
            .or_else(|| self.schema.action_by_key(symbol).map(|a| a.id)).unwrap()
    }
    fn set(&self, symbol: &str, value: ParameterValue) {
        self.device.lock().unwrap().values.insert(self.id(symbol), value);
    }
    fn enum_value(&self, symbol: &str, value: &str) -> u8 {
        self.schema.parameter_by_key(symbol).unwrap().enum_u8(value).unwrap()
    }
    fn finish(&self, handle: ActionHandle, status: AxdrStatus, valid: u8) {
        self.set("PARAM_MOTOR_STATE", ParameterValue::U8(self.enum_value("PARAM_MOTOR_STATE", "DISABLED")));
        self.set("PARAM_CAL_VALID", ParameterValue::U8(valid));
        self.device.lock().unwrap().incoming.push_back(CanFdFrame::new(can_id(MSG_EVENT, NODE_ID_DEFAULT), &[
            EVENT_ACTION_COMPLETE, handle.txn.get(), handle.action_id as u8,
            (handle.action_id >> 8) as u8, status as u8,
        ]).unwrap());
    }
}
impl FrameTransport for Wire {
    fn send(&mut self, frame: &CanFdFrame) -> Result<(), TransportError> {
        assert_eq!(frame.id, can_id(MSG_PARAMETER, NODE_ID_DEFAULT));
        let p = frame.data();
        let id = u16::from_le_bytes([p[2], p[3]]);
        let mut device = self.device.lock().unwrap();
        let mut response = vec![p[0], MSG_PARAMETER, p[1], AxdrStatus::Ok as u8, p[2], p[3]];
        if p[1] == PARAM_READ {
            let value = device.values[&id];
            response.push(value.parameter_type() as u8);
            match value {
                ParameterValue::U8(v) => response.push(v),
                ParameterValue::I8(v) => response.push(v as u8),
                ParameterValue::F32(v) => response.extend(v.to_le_bytes()),
                ParameterValue::U32(v) => response.extend(v.to_le_bytes()),
                ParameterValue::I32(v) => response.extend(v.to_le_bytes()),
                ParameterValue::Position(v) => { response.extend(v.turns.to_le_bytes()); response.extend(v.theta.to_le_bytes()); }
            }
        } else {
            assert_eq!(p[1], PARAM_WRITE);
            device.writes.push(id);
            if p[4] == ParameterType::Action as u8 {
                if id == self.id("ACTION_MOTOR_DISABLE") || id == self.id("ACTION_MOTOR_STOP") {
                    device.values.insert(self.id("PARAM_MOTOR_STATE"), ParameterValue::U8(0));
                } else {
                    assert!([self.id("ACTION_ENC_PHASE_START"), self.id("ACTION_ENC_CAL_START")].contains(&id));
                    assert_eq!(device.values[&self.id("PARAM_MOTOR_MODE")],
                        ParameterValue::U8(self.enum_value("PARAM_MOTOR_MODE", "ENCODER_CAL")));
                    assert_eq!(device.values[&self.id("PARAM_MOTOR_STATE")], ParameterValue::U8(0));
                    if device.reject_start { response[3] = AxdrStatus::ErrState as u8; }
                    else { device.values.insert(self.id("PARAM_MOTOR_STATE"), ParameterValue::U8(2)); }
                }
            } else {
                assert_eq!(id, self.id("PARAM_MOTOR_MODE"));
                device.values.insert(id, ParameterValue::U8(p[5]));
            }
        }
        device.incoming.push_back(CanFdFrame::new(can_id(MSG_RESPONSE, NODE_ID_DEFAULT), &response).unwrap());
        Ok(())
    }
    fn receive(&mut self, timeout: Duration) -> Result<CanFdFrame, TransportError> {
        if let Some(frame) = self.device.lock().unwrap().incoming.pop_front() { return Ok(frame); }
        thread::sleep(timeout.min(Duration::from_millis(1)));
        Err(TransportError::Timeout)
    }
}
fn setup() -> (ApplicationSession, Wire) {
    let schema = HostSchema::parse(include_str!("../tests/fixtures/axdr-host-schema.toml")).unwrap();
    let values = schema.parameters.iter().map(|meta| (meta.id, match meta.parameter_type().unwrap() {
        ParameterType::U8 => ParameterValue::U8(1),
        ParameterType::I8 => ParameterValue::I8(1),
        ParameterType::F32 => ParameterValue::F32(1.0),
        ParameterType::U32 => ParameterValue::U32(0),
        ParameterType::I32 => ParameterValue::I32(0),
        ParameterType::Position => ParameterValue::Position(PositionValue { turns: 0, theta: 0.0 }),
        _ => panic!("unexpected parameter type"),
    })).collect();
    let wire = Wire { schema: schema.clone(), device: Arc::new(Mutex::new(Device {
        values, incoming: VecDeque::new(), writes: vec![], reject_start: false,
    })) };
    for symbol in ["PARAM_MOTOR_STATE", "PARAM_MOTOR_MODE", "PARAM_ENCODER_FAULT"] { wire.set(symbol, ParameterValue::U8(0)); }
    wire.set("PARAM_LIMIT_WM_EFFECTIVE", ParameterValue::F32(200.0));
    let app = ApplicationSession::from_session(DeviceSession::spawn(Box::new(wire.clone())), schema).unwrap();
    (app, wire)
}

#[test]
fn both_encoder_tasks_select_mode_and_use_only_the_dedicated_action() {
    for calibration in [false, true] {
        for enabled in [false, true] {
            let (app, wire) = setup();
            if enabled { wire.set("PARAM_MOTOR_STATE", ParameterValue::U8(1)); }
            let completion = app.action_completion_waiter().unwrap();
            let handle = if calibration { app.encoder_calibration_start() } else { app.phase_search_start() }.unwrap();
            let mut expected = if enabled { vec![wire.id("ACTION_MOTOR_DISABLE")] } else { vec![] };
            expected.extend([wire.id("PARAM_MOTOR_MODE"), wire.id(if calibration { "ACTION_ENC_CAL_START" } else { "ACTION_ENC_PHASE_START" })]);
            wire.finish(handle, AxdrStatus::Ok, if calibration { 2 } else { 1 });
            assert_eq!(completion.wait(handle, Some(Duration::from_secs(5))).unwrap(), AxdrStatus::Ok);
            assert_eq!(app.motor_state().unwrap(), MotorState::Disabled);
            assert_eq!(wire.device.lock().unwrap().writes, expected);
        }
    }
}

#[test]
fn encoder_preflight_blocks_unaligned_or_under_speed_limit_without_any_writes() {
    for (symbol, value) in [("PARAM_CAL_VALID", ParameterValue::U8(0)),
        ("PARAM_LIMIT_WM_EFFECTIVE", ParameterValue::F32(100.0)),
        ("PARAM_MOTOR_FLUX", ParameterValue::F32(0.0)),
        ("PARAM_MOTION_WM_DEC", ParameterValue::F32(0.0))] {
        let (app, wire) = setup(); wire.set(symbol, value);
        assert!(app.encoder_calibration_start().is_err(), "{symbol}");
        assert!(wire.device.lock().unwrap().writes.is_empty());
    }
    let (app, wire) = setup();
    wire.set("PARAM_LIMIT_WM_EFFECTIVE", ParameterValue::F32(1000.0 * std::f32::consts::TAU / 60.0));
    assert!(app.encoder_calibration_start().is_ok());
}

#[test]
fn encoder_run_and_rejected_start_are_not_reported_as_accepted() {
    let (app, wire) = setup();
    wire.set("PARAM_MOTOR_STATE", ParameterValue::U8(2));
    assert!(app.encoder_calibration_start().is_err());
    assert!(app.phase_search_start().is_err());
    assert!(wire.device.lock().unwrap().writes.is_empty());
    wire.set("PARAM_MOTOR_STATE", ParameterValue::U8(0));
    wire.device.lock().unwrap().reject_start = true;
    assert!(app.encoder_calibration_start().is_err());
}

#[test]
fn encoder_completion_requires_the_full_saved_result() {
    let (app, wire) = setup();
    let completion = app.action_completion_waiter().unwrap();
    let handle = app.encoder_calibration_start().unwrap();
    wire.finish(handle, AxdrStatus::Ok, 1); // phase only is not full calibration
    assert!(matches!(completion.wait(handle, Some(Duration::from_secs(5))),
        Err(crate::ActionCompletionError::VerificationFailed(_))));
    let handle = app.phase_search_start().unwrap();
    wire.finish(handle, AxdrStatus::ErrConfig, 0);
    assert_eq!(completion.wait(handle, Some(Duration::from_secs(5))).unwrap(), AxdrStatus::ErrConfig);
}
