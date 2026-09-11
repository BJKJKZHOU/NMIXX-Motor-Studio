use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmixx_app::{AxdrStatus, DeviceSession, ParameterType, ParameterValue, SessionError, SessionEvent};
use nmixx_core::protocol::{
    EVENT_ACTION_COMPLETE, EVENT_NOTIFY, MSG_EVENT, MSG_PARAMETER, MSG_RESPONSE, NODE_ID_DEFAULT,
    PARAM_READ, PARAM_WRITE, can_id,
};
use nmixx_core::transport::{FrameTransport, TransportError};
use nmixx_core::wire::CanFdFrame;

#[derive(Clone, Default)]
struct ScriptState {
    incoming: Arc<Mutex<VecDeque<Result<CanFdFrame, TransportError>>>>,
    sent: Arc<Mutex<Vec<CanFdFrame>>>,
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
    state
        .incoming
        .lock()
        .unwrap()
        .push_back(Ok(response(1, PARAM_READ, AxdrStatus::Ok, &read_data)));

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
    state
        .incoming
        .lock()
        .unwrap()
        .push_back(Ok(response(1, PARAM_WRITE, AxdrStatus::Ok, &[0x01, 0x01])));

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
fn async_event_is_dispatched_while_waiting_for_parameter_response() {
    let state = ScriptState::default();
    state.incoming.lock().unwrap().push_back(Ok(
        CanFdFrame::new(
            can_id(MSG_EVENT, NODE_ID_DEFAULT),
            &[EVENT_NOTIFY, 0, 0, 0],
        )
        .unwrap(),
    ));
    state
        .incoming
        .lock()
        .unwrap()
        .push_back(Ok(response(
            1,
            PARAM_READ,
            AxdrStatus::Ok,
            &[0x10, 0x07, ParameterType::U8 as u8, 0],
        )));

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
fn action_completion_can_arrive_before_accept_response() {
    let state = ScriptState::default();
    let action_id = 0x1001;
    state
        .incoming
        .lock()
        .unwrap()
        .push_back(Ok(action_complete(1, action_id, AxdrStatus::Ok)));
    state
        .incoming
        .lock()
        .unwrap()
        .push_back(Ok(response(
            1,
            PARAM_WRITE,
            AxdrStatus::Ok,
            &[action_id as u8, (action_id >> 8) as u8],
        )));

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
fn timeout_cancels_pending_transaction_and_next_request_can_succeed() {
    let state = ScriptState::default();
    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state.clone())));

    assert!(matches!(
        session.parameter_read(0x0710, ParameterType::U8),
        Err(SessionError::Timeout)
    ));

    state
        .incoming
        .lock()
        .unwrap()
        .push_back(Ok(response(
            2,
            PARAM_READ,
            AxdrStatus::Ok,
            &[0x10, 0x07, ParameterType::U8 as u8, 0],
        )));

    assert_eq!(
        session.parameter_read(0x0710, ParameterType::U8).unwrap(),
        ParameterValue::U8(0)
    );
}
