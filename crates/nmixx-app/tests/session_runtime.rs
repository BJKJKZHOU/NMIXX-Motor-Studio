use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmixx_app::{AxdrStatus, ParameterType, ParameterValue, PositionValue};
use nmixx_app::raw::{ActionError, DeviceSession, SessionError, SessionEvent};
use nmixx_core::protocol::{
    EVENT_ACTION_COMPLETE, EVENT_NOTIFY, MSG_EVENT, MSG_PARAMETER, MSG_RESPONSE, NODE_ID_DEFAULT,
    PARAM_READ, PARAM_WRITE, can_id,
};
use nmixx_core::transport::{FrameTransport, TransportError};
use nmixx_core::wire::CanFdFrame;

#[derive(Clone, Default)]
struct ScriptState {
    incoming: Arc<Mutex<VecDeque<Result<CanFdFrame, TransportError>>>>,
    on_send: Arc<Mutex<VecDeque<Vec<Result<CanFdFrame, TransportError>>>>>,
    sent: Arc<Mutex<Vec<CanFdFrame>>>,
}

impl ScriptState {
    fn respond_on_send(&self, frames: Vec<Result<CanFdFrame, TransportError>>) {
        self.on_send.lock().unwrap().push_back(frames);
    }
}

struct ScriptedTransport {
    state: ScriptState,
}

impl ScriptedTransport {
    fn new(state: ScriptState) -> Self {
        Self { state }
    }
}

impl FrameTransport for ScriptedTransport {
    fn send(&mut self, frame: &CanFdFrame) -> Result<(), TransportError> {
        self.state.sent.lock().unwrap().push(frame.clone());
        if let Some(frames) = self.state.on_send.lock().unwrap().pop_front() {
            self.state.incoming.lock().unwrap().extend(frames);
        }
        Ok(())
    }

    fn receive(&mut self, _timeout: Duration) -> Result<CanFdFrame, TransportError> {
        self.state
            .incoming
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(TransportError::Timeout))
    }
}

fn response(txn: u8, op: u8, status: AxdrStatus, data: &[u8]) -> CanFdFrame {
    let mut payload = vec![txn, MSG_PARAMETER, op, status as u8];
    payload.extend_from_slice(data);
    CanFdFrame::new(can_id(MSG_RESPONSE, NODE_ID_DEFAULT), &payload).unwrap()
}

fn action_complete(txn: u8, action_id: u16, status: AxdrStatus) -> CanFdFrame {
    CanFdFrame::new(
        can_id(MSG_EVENT, NODE_ID_DEFAULT),
        &[
            EVENT_ACTION_COMPLETE,
            txn,
            action_id as u8,
            (action_id >> 8) as u8,
            status as u8,
        ],
    )
    .unwrap()
}

#[test]
fn parameter_read_round_trip_uses_session_runtime() {
    let state = ScriptState::default();
    let mut read_data = vec![0x10, 0x01, ParameterType::F32 as u8];
    read_data.extend_from_slice(&0.147f32.to_le_bytes());
    state.respond_on_send(vec![Ok(response(1, PARAM_READ, AxdrStatus::Ok, &read_data))]);

    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state.clone())));
    let value = session
        .parameter_read(0x0110, ParameterType::F32)
        .unwrap();

    let ParameterValue::F32(value) = value else {
        panic!("expected f32 parameter value");
    };
    assert!((value - 0.147).abs() < 1.0e-6);

    let sent = state.sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].id, can_id(MSG_PARAMETER, NODE_ID_DEFAULT));
    assert_eq!(&sent[0].data()[..4], &[1, PARAM_READ, 0x10, 0x01]);
}

#[test]
fn parameter_write_round_trip_uses_session_runtime() {
    let state = ScriptState::default();
    state.respond_on_send(vec![Ok(response(1, PARAM_WRITE, AxdrStatus::Ok, &[0x01, 0x01]))]);

    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state.clone())));
    session
        .parameter_write(0x0101, ParameterValue::U8(16))
        .unwrap();

    let sent = state.sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        &sent[0].data()[..6],
        &[1, PARAM_WRITE, 0x01, 0x01, ParameterType::U8 as u8, 16]
    );
}

#[test]
fn manual_position_repeat_sends_one_action_then_only_new_position_target() {
    let state = ScriptState::default();
    state.respond_on_send(vec![Ok(response(
        1, PARAM_WRITE, AxdrStatus::Ok, &[0x04, 0x07],
    ))]);
    state.respond_on_send(vec![Ok(response(
        2, PARAM_WRITE, AxdrStatus::Ok, &[0x02, 0x10],
    ))]);
    state.respond_on_send(vec![Ok(response(
        3, PARAM_WRITE, AxdrStatus::Ok, &[0x04, 0x07],
    ))]);

    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state.clone())));
    session.parameter_write(0x0704, ParameterValue::Position(
        PositionValue { turns: 2, theta: 0.0 },
    )).unwrap();
    let handle = session.action_start(0x1002).unwrap();
    assert_eq!(handle.action_id, 0x1002);
    // Once Position remains RUN the Host's next manual Repeat click updates
    // only the target. There must be no second ACTION_MOTOR_RUN on the wire.
    session.parameter_write(0x0704, ParameterValue::Position(
        PositionValue { turns: 1, theta: 0.0 },
    )).unwrap();

    let sent = state.sent.lock().unwrap();
    assert_eq!(sent.len(), 3);
    assert!(sent.iter().all(|frame| frame.id == can_id(MSG_PARAMETER, NODE_ID_DEFAULT)));
    assert_eq!(
        &sent[0].data()[..5],
        &[1, PARAM_WRITE, 0x04, 0x07, ParameterType::Position as u8]
    );
    assert_eq!(&sent[0].data()[5..9], &2i32.to_le_bytes());
    assert_eq!(
        &sent[1].data()[..5],
        &[2, PARAM_WRITE, 0x02, 0x10, ParameterType::Action as u8]
    );
    assert_eq!(
        &sent[2].data()[..5],
        &[3, PARAM_WRITE, 0x04, 0x07, ParameterType::Position as u8]
    );
    assert_eq!(&sent[2].data()[5..9], &1i32.to_le_bytes());
}

#[test]
fn async_event_is_dispatched_while_waiting_for_parameter_response() {
    let state = ScriptState::default();
    let event = CanFdFrame::new(
        can_id(MSG_EVENT, NODE_ID_DEFAULT),
        &[EVENT_NOTIFY, 0, 0, 0],
    )
    .unwrap();
    state.respond_on_send(vec![
        Ok(event),
        Ok(response(
            1,
            PARAM_READ,
            AxdrStatus::Ok,
            &[0x10, 0x07, ParameterType::U8 as u8, 0],
        )),
    ]);

    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state)));
    let events = session.subscribe().unwrap();
    let value = session.parameter_read(0x0710, ParameterType::U8).unwrap();
    assert_eq!(value, ParameterValue::U8(0));

    assert!(matches!(
        events.recv_timeout(Duration::from_millis(100)).unwrap(),
        SessionEvent::DeviceEvent(_)
    ));
}

#[test]
fn control_subscription_does_not_receive_waveform_frames() {
    use nmixx_core::protocol::MSG_FAST_DATA;

    let state = ScriptState::default();
    let stream = CanFdFrame::new(can_id(MSG_FAST_DATA, NODE_ID_DEFAULT), &[1, 0, 7, 1, 10, 0]).unwrap();
    let notify = CanFdFrame::new(can_id(MSG_EVENT, NODE_ID_DEFAULT), &[EVENT_NOTIFY, 0, 0, 0]).unwrap();
    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state.clone())));
    let control = session.subscribe_control().unwrap();
    state.incoming.lock().unwrap().extend([Ok(stream), Ok(notify)]);
    assert!(matches!(control.recv_timeout(Duration::from_millis(100)).unwrap(), SessionEvent::DeviceEvent(_)));
    assert!(control.try_recv().is_err());
}

#[test]
fn action_completion_can_arrive_before_accept_response() {
    let state = ScriptState::default();
    let action_id = 0x1001;
    state.respond_on_send(vec![
        Ok(action_complete(1, action_id, AxdrStatus::Ok)),
        Ok(response(
            1,
            PARAM_WRITE,
            AxdrStatus::Ok,
            &[action_id as u8, (action_id >> 8) as u8],
        )),
    ]);

    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state)));
    let events = session.subscribe().unwrap();
    let handle = session.action_start(action_id).unwrap();

    match events.recv_timeout(Duration::from_millis(100)).unwrap() {
        SessionEvent::ActionCompleted {
            handle: completed,
            status,
        } => {
            assert_eq!(completed, handle);
            assert_eq!(status, AxdrStatus::Ok);
        }
        other => panic!("unexpected session event: {other:?}"),
    }
}

#[test]
fn action_rejection_preserves_domain_status() {
    let state = ScriptState::default();
    let action_id = 0x1103;
    state.respond_on_send(vec![Ok(response(
        1,
        PARAM_WRITE,
        AxdrStatus::ErrState,
        &[action_id as u8, (action_id >> 8) as u8],
    ))]);

    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state)));
    assert!(matches!(
        session.action_start(action_id),
        Err(SessionError::Action(ActionError::Status(AxdrStatus::ErrState)))
    ));
}

#[test]
fn timeout_cancels_pending_transaction_and_next_request_can_succeed() {
    let state = ScriptState::default();
    state.respond_on_send(vec![]);
    state.respond_on_send(vec![Ok(response(
        2,
        PARAM_READ,
        AxdrStatus::Ok,
        &[0x10, 0x07, ParameterType::U8 as u8, 0],
    ))]);
    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state)));

    assert!(matches!(
        session.parameter_read(0x0710, ParameterType::U8),
        Err(SessionError::Timeout)
    ));

    assert_eq!(
        session.parameter_read(0x0710, ParameterType::U8).unwrap(),
        ParameterValue::U8(0)
    );
}

#[test]
fn malformed_response_does_not_poison_following_request() {
    let state = ScriptState::default();
    let malformed = CanFdFrame::new(
        can_id(MSG_RESPONSE, NODE_ID_DEFAULT),
        &[1, MSG_PARAMETER, PARAM_READ],
    )
    .unwrap();
    state.respond_on_send(vec![Ok(malformed)]);
    state.respond_on_send(vec![Ok(response(
        2,
        PARAM_READ,
        AxdrStatus::Ok,
        &[0x10, 0x07, ParameterType::U8 as u8, 0],
    ))]);

    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state)));
    assert!(matches!(
        session.parameter_read(0x0710, ParameterType::U8),
        Err(SessionError::Decode(_))
    ));

    assert_eq!(
        session.parameter_read(0x0710, ParameterType::U8).unwrap(),
        ParameterValue::U8(0)
    );
}

#[test]
fn permanent_transport_io_failure_closes_the_session_worker() {
    let state = ScriptState::default();
    state.incoming.lock().unwrap().push_back(Err(TransportError::Io("cable removed".into())));
    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state)));
    // The worker must terminate after the permanent I/O failure rather than
    // consuming further requests on the dead transport.
    let mut closed = false;
    for _ in 0..100 {
        if matches!(session.parameter_read(0x0710, ParameterType::U8), Err(SessionError::Closed)) {
            closed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(closed);
}
