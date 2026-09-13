use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmixx_app::{DevicePlotCapabilities, DeviceSession};
use nmixx_core::protocol::{
    AxdrStatus, MSG_PLOT, MSG_RESPONSE, NODE_ID_DEFAULT, PLOT_CAPS, PLOT_CAP_END,
    PLOT_CAP_FAST, PLOT_CAP_NORMAL, can_id,
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

fn caps_response(
    txn: u8,
    start: u8,
    next: u8,
    entries: &[(u16, u8, f32)],
) -> CanFdFrame {
    let mut payload = vec![txn, MSG_PLOT, PLOT_CAPS, AxdrStatus::Ok as u8];
    payload.extend_from_slice(&[start, next, entries.len() as u8, 8, 15, 20]);
    payload.extend_from_slice(&20_000u32.to_le_bytes());
    payload.extend_from_slice(&1_000u32.to_le_bytes());
    for (id, modes, scale) in entries {
        payload.extend_from_slice(&id.to_le_bytes());
        payload.push(*modes);
        payload.extend_from_slice(&scale.to_le_bytes());
    }
    CanFdFrame::new(can_id(MSG_RESPONSE, NODE_ID_DEFAULT), &payload).unwrap()
}

#[test]
fn discovers_all_plot_capability_pages() {
    let state = ScriptState::default();
    state.respond_on_send(vec![Ok(caps_response(
        1,
        0,
        2,
        &[
            (0x0001, PLOT_CAP_FAST | PLOT_CAP_NORMAL, 0.001),
            (0x0004, PLOT_CAP_NORMAL, 0.0),
        ],
    ))]);
    state.respond_on_send(vec![Ok(caps_response(
        2,
        2,
        PLOT_CAP_END,
        &[(0x0011, PLOT_CAP_FAST | PLOT_CAP_NORMAL, 0.001)],
    ))]);

    let session = DeviceSession::spawn(Box::new(ScriptedTransport::new(state.clone())));
    let capabilities = DevicePlotCapabilities::discover(&session).unwrap();

    assert_eq!(capabilities.fast_max_channels, 8);
    assert_eq!(capabilities.normal_max_channels, 15);
    assert_eq!(capabilities.fast_block_samples, 20);
    assert_eq!(capabilities.fast_rate_hz, 20_000);
    assert_eq!(capabilities.normal_rate_hz, 1_000);
    assert_eq!(capabilities.channels.len(), 3);
    assert!(capabilities.channel(0x0001).unwrap().supports_fast());
    assert!(!capabilities.channel(0x0004).unwrap().supports_fast());
    assert!(capabilities.channel(0x0004).unwrap().supports_normal());

    let sent = state.sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    assert_eq!(&sent[0].data()[..3], &[1, PLOT_CAPS, 0]);
    assert_eq!(&sent[1].data()[..3], &[2, PLOT_CAPS, 2]);
}
