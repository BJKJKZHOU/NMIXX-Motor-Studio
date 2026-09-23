//! Transport-level regression: actual decoder and shared records, no motor I/O.
use super::*;
use std::collections::VecDeque;
use std::time::Instant;
use nmixx_core::protocol::{can_id, AxdrStatus, MSG_NORMAL_DATA, MSG_PLOT, MSG_RESPONSE,
    NODE_ID_DEFAULT, PLOT_CONFIG, PLOT_GROUP_NORMAL};
use nmixx_core::transport::{FrameTransport, TransportError};
use nmixx_core::wire::CanFdFrame;

#[derive(Clone, Default)]
struct Wire {
    incoming: Arc<Mutex<VecDeque<CanFdFrame>>>,
    sent: Arc<Mutex<Vec<CanFdFrame>>>,
    normal: Arc<Mutex<(u8, Vec<u16>)>>,
}
impl FrameTransport for Wire {
    fn send(&mut self, frame: &CanFdFrame) -> Result<(), TransportError> {
        // Baseline/Scope/Tuning acquisition must never issue a motor Action.
        assert_eq!(frame.id, can_id(MSG_PLOT, NODE_ID_DEFAULT));
        self.sent.lock().unwrap().push(frame.clone());
        let bytes = frame.data();
        let mut reply = vec![bytes[0], MSG_PLOT, bytes[1], AxdrStatus::Ok as u8];
        if bytes[1] == PLOT_CONFIG {
            let end = 5 + usize::from(bytes[4]) * 2;
            reply.extend_from_slice(&bytes[2..end]);
            if bytes[2] == PLOT_GROUP_NORMAL {
                *self.normal.lock().unwrap() = (bytes[3], bytes[5..end].chunks_exact(2)
                    .map(|id| u16::from_le_bytes([id[0], id[1]])).collect());
            }
        }
        self.incoming.lock().unwrap().push_back(CanFdFrame::new(can_id(MSG_RESPONSE, NODE_ID_DEFAULT), &reply).unwrap());
        Ok(())
    }
    fn receive(&mut self, timeout: Duration) -> Result<CanFdFrame, TransportError> {
        if let Some(frame) = self.incoming.lock().unwrap().pop_front() { return Ok(frame); }
        std::thread::sleep(timeout.min(Duration::from_millis(1)));
        Err(TransportError::Timeout)
    }
}
impl Wire {
    fn sample(&self, sequence: u16, value: f32) {
        let (config, ids) = self.normal.lock().unwrap().clone();
        let mut payload = vec![sequence as u8, (sequence >> 8) as u8, config, ids.len() as u8];
        for _ in ids { payload.extend_from_slice(&value.to_le_bytes()); }
        self.incoming.lock().unwrap().push_back(CanFdFrame::new(can_id(MSG_NORMAL_DATA, NODE_ID_DEFAULT), &payload).unwrap());
    }
}
fn wait_value(parameters: &ParameterService, value: f32) {
    let deadline = Instant::now() + Duration::from_secs(1);
    while parameters.cached(17).unwrap() != Some(crate::ParameterValue::F32(value)) {
        assert!(Instant::now() < deadline, "shared stream did not publish the sample");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn schema() -> HostSchema {
    let mut text = String::from("schema_version=1\nprotocol=\"axdr-canfd-v1\"\n[source]\nrepository=\"test\"\ngit_sha=\"test\"\nparameter_schema=1\n");
    for (id, symbol, ty) in [(4, "PARAM_ADC_VBUS", "f32"), (17, "PARAM_RUN_IQ", "f32"),
        (18, "PARAM_RUN_UD", "f32"), (1281, "PARAM_RUN_POSITION", "position")] {
        text.push_str(&format!("\n[[parameters]]\nid={id}\nsymbol=\"{symbol}\"\nlabel=\"{symbol}\"\ntype=\"{ty}\"\naccess=\"ro\"\ndescription=\"test\"\n"));
    }
    HostSchema::parse(&text).unwrap()
}
#[test]
fn baseline_visibility_and_independent_records_share_one_wire_stream() {
    let wire = Wire::default();
    let session = DeviceSession::spawn(Box::new(wire.clone()));
    let schema = schema();
    let parameters = ParameterService::new(session.clone(), schema.clone());
    let caps = DevicePlotCapabilities {
        fast_max_channels: 8, normal_max_channels: 15, fast_block_samples: 20,
        fast_rate_hz: 20_000, normal_rate_hz: 1000,
        channels: [4, 17, 18, 1281].iter().map(|&id| crate::DevicePlotChannel {
            id, modes: crate::PLOT_CAP_NORMAL, fast_scale: 0.0,
        }).collect(),
    };
    let source = SharedAcquisition::new(session, caps, schema, parameters.clone()).unwrap();
    let history = Duration::from_secs(10);
    assert_eq!(wire.sent.lock().unwrap().len(), 2); // One config, one start.
    wire.sample(0, 1.0); wait_value(&parameters, 1.0);
    source.configure(View::Scope, &[], history).unwrap();
    wire.sample(1, 2.0); wait_value(&parameters, 2.0);
    source.configure(View::Scope, &[ScopeSelection { id: 17, rate: ScopeRate::Normal }], history).unwrap();
    assert_eq!(source.snapshot(View::Scope, history, Duration::ZERO).unwrap().series[0].values, vec![1., 2.]);
    source.stop(View::Scope, StreamState::Stopped).unwrap();
    wire.sample(2, 3.0); wait_value(&parameters, 3.0);
    let frozen = source.snapshot(View::Scope, history, Duration::ZERO).unwrap();
    assert_eq!(frozen.series[0].values, vec![1., 2.]);
    // Typed position remains a real Parameter read, never f32 Plot reconstruction.
    assert_eq!(parameters.cached(1281).unwrap(), None);
    source.configure(View::Tuning, &[ScopeSelection { id: 17, rate: ScopeRate::Normal }], history).unwrap();
    source.capture(View::Tuning, history).unwrap();
    wire.sample(3, 4.0); wait_value(&parameters, 4.0);
    source.stop(View::Tuning, StreamState::Stopped).unwrap();
    wire.sample(4, 5.0); wait_value(&parameters, 5.0);
    assert_eq!(source.snapshot(View::Scope, history, Duration::ZERO).unwrap(), frozen);
    assert_eq!(source.snapshot(View::Tuning, history, Duration::ZERO).unwrap().series[0].values, vec![4.]);
    assert_eq!(wire.sent.lock().unwrap().len(), 2); // No duplicate Config/Start/Stop.
    // Changing a frozen view does not touch the wire; Run allocates the new layout.
    source.configure(View::Scope, &[ScopeSelection { id: 18, rate: ScopeRate::Normal }], history).unwrap();
    assert_eq!(wire.sent.lock().unwrap().len(), 2);
    source.live(View::Scope).unwrap();
    assert_eq!(wire.sent.lock().unwrap().len(), 3);
    wire.sample(5, 6.0); wait_value(&parameters, 6.0);
    assert_eq!(source.snapshot(View::Scope, history, Duration::ZERO).unwrap().series[0].values, vec![6.]);
    assert_eq!(source.snapshot(View::Tuning, history, Duration::ZERO).unwrap().series[0].values, vec![4.]);
}
