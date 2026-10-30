//! Exercise completion cleanup through the real session decoder and Application.
use super::*;
use std::collections::VecDeque;
use nmixx_core::protocol::{can_id, ParameterType, EVENT_ACTION_COMPLETE, MSG_EVENT,
    MSG_PARAMETER, MSG_RESPONSE, NODE_ID_DEFAULT, PARAM_READ, PARAM_WRITE};
use nmixx_core::transport::{FrameTransport, TransportError};
use nmixx_core::wire::CanFdFrame;

#[derive(Clone, Copy, Default)]
enum DisableBehavior { #[default] Normal, Reject, StayEnabled, ReadFailure }

struct Device {
    incoming: VecDeque<CanFdFrame>,
    writes: Vec<u16>,
    state: u8,
    disable: DisableBehavior,
    disable_attempted: bool,
    applied: bool,
}
#[derive(Clone)]
struct Wire { schema: HostSchema, device: Arc<Mutex<Device>> }
impl Wire {
    fn id(&self, symbol: &str) -> u16 {
        self.schema.parameter_by_symbol(symbol).map(|p| p.id)
            .or_else(|| self.schema.action_by_key(symbol).map(|a| a.id)).unwrap()
    }
    fn state(&self, symbol: &str) -> u8 {
        self.schema.parameter_by_symbol("PARAM_MOTOR_STATE").unwrap().enum_u8(symbol).unwrap()
    }
    fn finish(&self, handle: ActionHandle, status: AxdrStatus) {
        let mut device = self.device.lock().unwrap();
        device.state = self.state("ENABLED");
        device.incoming.push_back(CanFdFrame::new(can_id(MSG_EVENT, NODE_ID_DEFAULT), &[
            EVENT_ACTION_COMPLETE, handle.txn.get(), handle.action_id as u8,
            (handle.action_id >> 8) as u8, status as u8,
        ]).unwrap());
    }
}
impl FrameTransport for Wire {
    fn send(&mut self, frame: &CanFdFrame) -> Result<(), TransportError> {
        assert_eq!(frame.id, can_id(MSG_PARAMETER, NODE_ID_DEFAULT));
        let bytes = frame.data();
        let id = u16::from_le_bytes([bytes[2], bytes[3]]);
        let mut device = self.device.lock().unwrap();
        let mut status = AxdrStatus::Ok;
        let mut data = id.to_le_bytes().to_vec();
        if bytes[1] == PARAM_WRITE {
            // Cleanup and Apply must never write the mode or issue Enable/Run.
            assert_eq!(bytes[4], ParameterType::Action as u8);
            device.writes.push(id);
            if id == self.id("ACTION_MOTOR_DISABLE") {
                device.disable_attempted = true;
                match device.disable {
                    DisableBehavior::Reject => status = AxdrStatus::ErrState,
                    DisableBehavior::StayEnabled => {},
                    _ => device.state = self.state("DISABLED"),
                }
            } else if id == self.id("ACTION_IDENT_APPLY") {
                assert_eq!(device.state, self.state("DISABLED"));
                device.applied = true;
            }
        } else {
            assert_eq!(bytes[1], PARAM_READ);
            let meta = self.schema.parameters.iter().find(|p| p.id == id).unwrap();
            let ty = meta.parameter_type().unwrap();
            data.push(ty as u8);
            match ty {
                ParameterType::U8 | ParameterType::I8 => {
                    data.push(match meta.symbol.as_str() {
                        "PARAM_MOTOR_STATE" => {
                            if device.disable_attempted && matches!(device.disable, DisableBehavior::ReadFailure) {
                                status = AxdrStatus::ErrState;
                            }
                            device.state
                        },
                        "PARAM_MOTOR_MODE" => meta.enum_u8("IDENT").unwrap(),
                        "PARAM_IDENT_RS_LS_VALID" | "PARAM_IDENT_FLUX_VALID" | "PARAM_IDENT_JB_VALID" => 1,
                        _ => 0,
                    });
                }
                ParameterType::F32 | ParameterType::U32 | ParameterType::I32 => data.extend([0; 4]),
                ParameterType::Position => data.extend([0; 8]),
                _ => panic!("unexpected readable type"),
            }
        }
        let mut response = vec![bytes[0], MSG_PARAMETER, bytes[1], status as u8];
        response.extend(data);
        device.incoming.push_back(CanFdFrame::new(can_id(MSG_RESPONSE, NODE_ID_DEFAULT), &response).unwrap());
        Ok(())
    }
    fn receive(&mut self, timeout: Duration) -> Result<CanFdFrame, TransportError> {
        if let Some(frame) = self.device.lock().unwrap().incoming.pop_front() { return Ok(frame); }
        thread::sleep(timeout.min(Duration::from_millis(1)));
        Err(TransportError::Timeout)
    }
}
fn setup(disable: DisableBehavior) -> (ApplicationSession, Wire) {
    let schema = HostSchema::parse(include_str!("../tests/fixtures/axdr-host-schema.toml")).unwrap();
    let wire = Wire { schema: schema.clone(), device: Arc::new(Mutex::new(Device {
        incoming: VecDeque::new(), writes: Vec::new(), state: 0, disable,
        disable_attempted: false, applied: false,
    })) };
    let app = ApplicationSession::from_session(DeviceSession::spawn(Box::new(wire.clone())), schema).unwrap();
    (app, wire)
}
fn complete(app: &ApplicationSession, wire: &Wire, symbol: &str, status: AxdrStatus)
    -> Result<AxdrStatus, crate::ActionCompletionError>
{
    let waiter = app.action_completion_waiter().unwrap();
    let handle = app.inner.session.action_start(wire.id(symbol)).unwrap();
    wire.finish(handle, status);
    waiter.wait(handle, Some(Duration::from_secs(5)))
}

#[test]
fn identification_completion_disables_preserves_mode_and_allows_apply() {
    for (symbol, kind) in [
        ("ACTION_IDENT_RS_LS_START", IdentificationKind::RsLs),
        ("ACTION_IDENT_FLUX_START", IdentificationKind::Flux),
        ("ACTION_IDENT_JB_START", IdentificationKind::Jb),
    ] {
        let (app, wire) = setup(DisableBehavior::Normal);
        // Automatic cleanup must not revoke a script's workflow ownership.
        let (workflow, cancelled) = app.automation_claim().unwrap();
        assert_eq!(complete(&app, &wire, symbol, AxdrStatus::Ok).unwrap(), AxdrStatus::Ok);
        assert!(!cancelled.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(app.inner.parameters.cached(wire.id("PARAM_MOTOR_STATE")).unwrap(),
            Some(ParameterValue::U8(wire.state("DISABLED"))));
        let mode = wire.schema.parameter_by_symbol("PARAM_MOTOR_MODE").unwrap();
        assert_eq!(app.inner.parameters.cached(mode.id).unwrap(), Some(ParameterValue::U8(mode.enum_u8("IDENT").unwrap())));
        assert_eq!(app.identification_apply_candidate().unwrap(), Some(kind));
        workflow.identification_apply_kind(kind).unwrap();
        assert!(wire.device.lock().unwrap().applied);
        assert_eq!(app.identification_apply_candidate().unwrap(), None);
        assert_eq!(wire.device.lock().unwrap().writes, vec![wire.id(symbol), wire.id("ACTION_MOTOR_DISABLE"), wire.id("ACTION_IDENT_APPLY")]);
        workflow.automation_release();
    }
}

#[test]
fn failed_identification_also_disables_without_an_apply_candidate() {
    let (app, wire) = setup(DisableBehavior::Normal);
    assert_eq!(complete(&app, &wire, "ACTION_IDENT_RS_LS_START", AxdrStatus::ErrState).unwrap(), AxdrStatus::ErrState);
    assert_eq!(app.motor_state().unwrap(), MotorState::Disabled);
    assert_eq!(app.identification_apply_candidate().unwrap(), None);
}

#[test]
fn disable_rejection_or_unconfirmed_state_is_reported_as_cleanup_failure() {
    for behavior in [DisableBehavior::Reject, DisableBehavior::StayEnabled, DisableBehavior::ReadFailure] {
        let (app, wire) = setup(behavior);
        assert!(matches!(complete(&app, &wire, "ACTION_IDENT_RS_LS_START", AxdrStatus::Ok),
            Err(crate::ActionCompletionError::CleanupFailed(message)) if message.contains("automatic Disable failed")));
    }
}

#[test]
fn other_action_completions_do_not_auto_disable() {
    let (app, wire) = setup(DisableBehavior::Normal);
    assert_eq!(complete(&app, &wire, "ACTION_MOTOR_RUN", AxdrStatus::Ok).unwrap(), AxdrStatus::Ok);
    assert_eq!(wire.device.lock().unwrap().writes, vec![wire.id("ACTION_MOTOR_RUN")]);
}
