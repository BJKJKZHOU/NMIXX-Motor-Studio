use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nmixx_app::{DeviceSession, SessionEvent};
use nmixx_core::protocol::{
    AxdrStatus, MSG_FAST_DATA, MSG_PLOT, MSG_RESPONSE, NODE_ID_DEFAULT, PLOT_CONFIG,
    PLOT_FAST_MASK, PLOT_GROUP_FAST, PLOT_START, PLOT_STOP, can_id,
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

fn plot_response(txn: u8, op: u8, data: &[u8]) -> CanFdFrame {
    let mut payload = vec![txn, MSG_PLOT, op, AxdrStatus::Ok as u8];
    payload.extend_from_slice(data);
    CanFdFrame::new(can_id(MSG_RESPONSE, NODE_ID_DEFAULT), &payload).unwrap()
}

fn fast_frame(sequence: u16) -> CanFdFrame {
    CanFdFrame::new(
        can_id(MSG_FAST_DATA, NODE_ID_DEFAULT),
        &[
            sequence as u8,
            (sequence >> 8) as u8,
            7,
            1,
            sequence as u8,
            0,
        ],
    )
    .unwrap()
}

#[test]
fn plot_config_start_stream_and_stop_share_single_session_owner() {
    let state = ScriptState::default();
    let parameters = [0x0001, 0x0011];

    state.incoming.lock().unwrap().push_back(Ok(plot_response(
        1,
        PLOT_CONFIG,
        &[PLOT_GROUP_FAST, 7, 2, 0x01, 0x00, 0x11, 0x00],
    )));
    state
        .incoming
        .lock()
        .unwrap()
        .push_back(Ok(plot_response(2, PLOT_START, &[])));

    let fast = CanFdFrame::new(
        can_id(MSG_FAST_DATA, NODE_ID_DEFAULT),
        &[1, 0, 7, 2, 10, 0, 236, 255, 20, 0, 216, 255],
    )
    .unwrap();
    state.incoming.lock().unwrap().push_back(Ok(fast.clone()));
    state
        .incoming
        .lock()
        .unwrap()
        .push_back(Ok(plot_response(3, PLOT_STOP, &[])));

    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state.clone())));
    let events = session.subscribe().unwrap();

    session
        .plot_config(PLOT_GROUP_FAST, 7, &parameters)
        .unwrap();
    session.plot_start(PLOT_FAST_MASK).unwrap();

    match events.recv_timeout(Duration::from_millis(100)).unwrap() {
        SessionEvent::FastData(frame) => assert_eq!(frame, fast),
        other => panic!("unexpected event: {other:?}"),
    }

    session.plot_stop(PLOT_FAST_MASK).unwrap();

    let sent = state.sent.lock().unwrap();
    assert_eq!(sent.len(), 3);
    assert_eq!(sent[0].id, can_id(MSG_PLOT, NODE_ID_DEFAULT));
    assert_eq!(
        &sent[0].data()[..9],
        &[1, PLOT_CONFIG, PLOT_GROUP_FAST, 7, 2, 0x01, 0x00, 0x11, 0x00]
    );
    assert_eq!(&sent[1].data()[..3], &[2, PLOT_START, PLOT_FAST_MASK]);
    assert_eq!(&sent[2].data()[..3], &[3, PLOT_STOP, PLOT_FAST_MASK]);
}

#[test]
fn fast_burst_is_dispatched_without_per_frame_poll_delay() {
    const FRAME_COUNT: usize = 8;

    let state = ScriptState::default();
    {
        let mut incoming = state.incoming.lock().unwrap();
        for sequence in 0..FRAME_COUNT as u16 {
            incoming.push_back(Ok(fast_frame(sequence)));
        }
    }

    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state)));
    let events = session.subscribe().unwrap();
    let start = Instant::now();

    for _ in 0..FRAME_COUNT {
        assert!(matches!(
            events.recv_timeout(Duration::from_millis(100)).unwrap(),
            SessionEvent::FastData(_)
        ));
    }

    // The previous worker inserted one 20 ms command wait before every frame,
    // so 8 frames required about 160 ms. Keep a generous threshold to avoid
    // making this a scheduler-sensitive microbenchmark while still catching
    // that regression.
    assert!(start.elapsed() < Duration::from_millis(100));
}
