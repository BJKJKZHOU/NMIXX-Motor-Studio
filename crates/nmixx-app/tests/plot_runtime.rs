use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nmixx_app::{
    DevicePlotCapabilities, DevicePlotChannel, HostSchema, MixedScopeError, ScopeRate, ScopeSelection,
};
use nmixx_app::raw::{DeviceSession, MixedScopeSession, SessionEvent};
use nmixx_core::protocol::{
    AxdrStatus, MSG_FAST_DATA, MSG_PLOT, MSG_RESPONSE, NODE_ID_DEFAULT, PLOT_CAP_FAST, PLOT_CONFIG,
    PLOT_FAST_MASK, PLOT_GROUP_FAST, PLOT_START, PLOT_STOP, can_id,
};
use nmixx_core::transport::{FrameTransport, TransportError};
use nmixx_core::wire::CanFdFrame;

#[derive(Clone, Default)]
struct ScriptState {
    incoming: Arc<Mutex<VecDeque<Result<CanFdFrame, TransportError>>>>,
    on_send: Arc<Mutex<VecDeque<Vec<Result<CanFdFrame, TransportError>>>>>,
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

fn fast_frame_with_config(sequence: u16, config_id: u8, raw: i16) -> CanFdFrame {
    let [lo, hi] = raw.to_le_bytes();
    CanFdFrame::new(
        can_id(MSG_FAST_DATA, NODE_ID_DEFAULT),
        &[sequence as u8, (sequence >> 8) as u8, config_id, 1, lo, hi],
    )
    .unwrap()
}

fn mixed_scope_context() -> (DevicePlotCapabilities, HostSchema, Vec<ScopeSelection>) {
    let capabilities = DevicePlotCapabilities {
        fast_max_channels: 8,
        normal_max_channels: 15,
        fast_block_samples: 20,
        fast_rate_hz: 20_000,
        normal_rate_hz: 1_000,
        channels: vec![
            DevicePlotChannel { id: 0x0001, modes: PLOT_CAP_FAST, fast_scale: 0.1 },
            DevicePlotChannel { id: 0x0011, modes: PLOT_CAP_FAST, fast_scale: 0.5 },
        ],
    };
    let schema = HostSchema::parse(r#"
schema_version = 1
protocol = "axdr-canfd-v1"

[source]
repository = "fixture"
git_sha = "abc"
parameter_schema = 1

[[parameters]]
symbol = "PARAM_ADC_IA"
label = "Ia"
id = 1
type = "f32"
access = "ro"
description = "Ia"

[[parameters]]
symbol = "PARAM_RUN_IQ"
label = "Iq"
id = 17
type = "f32"
access = "ro"
description = "Iq"
"#).unwrap();
    let selections = vec![
        ScopeSelection { id: 0x0001, rate: ScopeRate::Fast },
        ScopeSelection { id: 0x0011, rate: ScopeRate::Fast },
    ];
    (capabilities, schema, selections)
}

#[test]
fn plot_config_start_stream_and_stop_share_single_session_owner() {
    let state = ScriptState::default();
    let parameters = [0x0001, 0x0011];
    let fast = CanFdFrame::new(
        can_id(MSG_FAST_DATA, NODE_ID_DEFAULT),
        &[1, 0, 7, 2, 10, 0, 236, 255, 20, 0, 216, 255],
    )
    .unwrap();

    {
        let mut on_send = state.on_send.lock().unwrap();
        on_send.push_back(vec![Ok(plot_response(
            1,
            PLOT_CONFIG,
            &[PLOT_GROUP_FAST, 7, 2, 0x01, 0x00, 0x11, 0x00],
        ))]);
        on_send.push_back(vec![
            Ok(plot_response(2, PLOT_START, &[])),
            Ok(fast.clone()),
        ]);
        on_send.push_back(vec![Ok(plot_response(3, PLOT_STOP, &[]))]);
    }

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
fn scope_live_snapshot_pause_and_clear_follow_fast_samples() {
    let state = ScriptState::default();
    let fast = CanFdFrame::new(
        can_id(MSG_FAST_DATA, NODE_ID_DEFAULT),
        &[1, 0, 7, 2, 10, 0, 236, 255, 20, 0, 216, 255],
    )
    .unwrap();
    {
        let mut on_send = state.on_send.lock().unwrap();
        on_send.push_back(vec![Ok(plot_response(
            1,
            PLOT_CONFIG,
            &[PLOT_GROUP_FAST, 7, 2, 0x01, 0x00, 0x11, 0x00],
        ))]);
        on_send.push_back(vec![Ok(plot_response(2, PLOT_START, &[])), Ok(fast)]);
        on_send.push_back(vec![Ok(plot_response(3, PLOT_STOP, &[]))]);
    }

    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state.clone())));
    let (capabilities, schema, selections) = mixed_scope_context();
    let scope = MixedScopeSession::from_capabilities(
        session,
        &capabilities,
        &schema,
        &selections,
        Duration::from_secs(1),
        7,
    )
    .unwrap();

    scope.live().unwrap();
    let deadline = Instant::now() + Duration::from_millis(100);
    while scope.status().unwrap().samples < 4 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    let snapshot = scope.snapshot_tail(Duration::from_millis(1)).unwrap();
    assert_eq!(snapshot.series[0].values, vec![1.0, 2.0]);
    assert_eq!(snapshot.series[1].values, vec![-10.0, -20.0]);

    scope.pause().unwrap();
    assert_eq!(scope.status().unwrap().state, nmixx_app::StreamState::Paused);
    scope.clear().unwrap();
    assert_eq!(scope.status().unwrap().samples, 0);
    let sent = state.sent.lock().unwrap();
    assert_eq!(&sent[2].data()[..3], &[3, PLOT_STOP, PLOT_FAST_MASK]);
}

#[test]
fn live_hot_reconfigure_promotes_new_config_without_restarting_group() {
    let state = ScriptState::default();
    {
        let mut on_send = state.on_send.lock().unwrap();
        // Initial FAST layout: PARAM_ADC_IA @ config 7.
        on_send.push_back(vec![Ok(plot_response(
            1,
            PLOT_CONFIG,
            &[PLOT_GROUP_FAST, 7, 1, 0x01, 0x00],
        ))]);
        on_send.push_back(vec![Ok(plot_response(2, PLOT_START, &[]))]);
        // Live hot reconfigure: PARAM_RUN_IQ @ config 8.
        on_send.push_back(vec![Ok(plot_response(
            3,
            PLOT_CONFIG,
            &[PLOT_GROUP_FAST, 8, 1, 0x11, 0x00],
        ))]);
    }

    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state.clone())));
    let (capabilities, schema, _) = mixed_scope_context();
    let scope = MixedScopeSession::from_capabilities(
        session,
        &capabilities,
        &schema,
        &[ScopeSelection { id: 0x0001, rate: ScopeRate::Fast }],
        Duration::from_secs(1),
        7,
    ).unwrap();

    scope.live().unwrap();
    scope.reconfigure(&[ScopeSelection { id: 0x0011, rate: ScopeRate::Fast }]).unwrap();

    // An old-layout frame may already be in flight after the Config response.
    // It must not poison the runtime while pending config 8 is waiting.
    {
        let mut incoming = state.incoming.lock().unwrap();
        incoming.push_back(Ok(fast_frame_with_config(10, 7, 10)));
        incoming.push_back(Ok(fast_frame_with_config(11, 8, 4)));
    }

    let deadline = Instant::now() + Duration::from_millis(100);
    loop {
        let snapshot = scope.snapshot_tail(Duration::from_millis(10)).unwrap();
        if snapshot.series[0].values == vec![2.0] {
            assert_eq!(snapshot.series[0].id, 0x0011);
            break;
        }
        assert!(Instant::now() < deadline, "new hot-reconfigure layout was not promoted");
        std::thread::yield_now();
    }

    let sent = state.sent.lock().unwrap();
    assert_eq!(sent.len(), 3);
    assert_eq!(&sent[0].data()[..5], &[1, PLOT_CONFIG, PLOT_GROUP_FAST, 7, 1]);
    assert_eq!(&sent[1].data()[..3], &[2, PLOT_START, PLOT_FAST_MASK]);
    assert_eq!(&sent[2].data()[..7], &[3, PLOT_CONFIG, PLOT_GROUP_FAST, 8, 1, 0x11, 0x00]);
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

    assert!(start.elapsed() < Duration::from_millis(100));
}

#[test]
fn scope_surfaces_fast_ingest_failure_and_stops_plot() {
    let state = ScriptState::default();
    let invalid_fast = CanFdFrame::new(
        can_id(MSG_FAST_DATA, NODE_ID_DEFAULT),
        &[1, 0, 8, 1, 10, 0],
    )
    .unwrap();

    {
        let mut on_send = state.on_send.lock().unwrap();
        on_send.push_back(vec![Ok(plot_response(
            1,
            PLOT_CONFIG,
            &[PLOT_GROUP_FAST, 7, 1, 0x01, 0x00],
        ))]);
        on_send.push_back(vec![
            Ok(plot_response(2, PLOT_START, &[])),
            Ok(invalid_fast),
        ]);
        on_send.push_back(vec![Ok(plot_response(3, PLOT_STOP, &[]))]);
    }

    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state.clone())));
    let (mut capabilities, schema, _) = mixed_scope_context();
    capabilities.channels.truncate(1);
    let selections = [ScopeSelection { id: 0x0001, rate: ScopeRate::Fast }];
    let scope = MixedScopeSession::from_capabilities(
        session,
        &capabilities,
        &schema,
        &selections,
        Duration::from_millis(100),
        7,
    )
    .unwrap();

    scope.live().unwrap();

    let deadline = Instant::now() + Duration::from_millis(100);
    loop {
        match scope.status() {
            Err(MixedScopeError::Runtime(error)) => {
                assert!(error.contains("unknown Config_ID") || error.contains("Config_ID"));
                break;
            }
            Ok(_) if Instant::now() < deadline => std::thread::yield_now(),
            other => panic!("Scope runtime failure was not surfaced: {other:?}"),
        }
    }

    assert!(matches!(
        scope.snapshot_tail(Duration::from_millis(10)),
        Err(MixedScopeError::Runtime(_))
    ));

    let sent = state.sent.lock().unwrap();
    assert_eq!(&sent[2].data()[..3], &[3, PLOT_STOP, PLOT_FAST_MASK]);
}

