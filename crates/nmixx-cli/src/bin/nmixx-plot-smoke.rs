use std::error::Error;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::Parser;
use nmixx_app::{
    DEFAULT_USB_BAUD, DeviceSession, HostSchema, PLOT_FAST_MASK, PLOT_GROUP_FAST,
    SessionEvent, StreamConfig, StreamPipeline, StreamSession, StreamWireMode,
};

const CONFIG_ID: u8 = 1;
const DEFAULT_DURATION_MS: u64 = 1_000;
const DEFAULT_HISTORY_MS: u64 = 2_000;
const FAST_SAMPLE_RATE_HZ: u32 = 20_000;
const DEFAULT_CHANNELS: [&str; 2] = ["PARAM_ADC_IA", "PARAM_RUN_IQ"];

#[derive(Debug, Parser)]
#[command(
    name = "nmixx-plot-smoke",
    about = "Minimal AxDr_L FAST Plot end-to-end smoke test"
)]
struct Args {
    /// AxDr_L USB CDC serial port, e.g. /dev/ttyACM0 or COM7.
    #[arg(long)]
    port: String,

    /// Host schema exported from the same firmware parameter.yaml.
    #[arg(long)]
    schema: PathBuf,

    /// Capture duration in milliseconds.
    #[arg(long, default_value_t = DEFAULT_DURATION_MS)]
    duration_ms: u64,

    /// Serial line rate used by the host serial API.
    #[arg(long, default_value_t = DEFAULT_USB_BAUD)]
    baud: u32,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("FAIL: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let schema = HostSchema::load(&args.schema)?;

    let mut ids = Vec::with_capacity(DEFAULT_CHANNELS.len());
    let mut scales = Vec::with_capacity(DEFAULT_CHANNELS.len());
    for key in DEFAULT_CHANNELS {
        let parameter = schema
            .parameter_by_key(key)
            .ok_or_else(|| format!("{key} is missing from HostSchema"))?;
        let scale = parameter
            .plot_scale
            .ok_or_else(|| format!("{key} has no plot_scale and cannot be used for FAST Plot"))?;
        if scale <= 0.0 {
            return Err(format!("{key} has invalid FAST plot_scale={scale}").into());
        }
        ids.push(parameter.id);
        scales.push(scale as f32);
    }

    let history_ms = args.duration_ms.max(DEFAULT_HISTORY_MS);
    let stream = StreamSession::new(StreamConfig {
        sample_rate_hz: FAST_SAMPLE_RATE_HZ,
        channel_count: ids.len(),
        history: Duration::from_millis(history_ms),
    })?;
    let mut pipeline = StreamPipeline::new(
        CONFIG_ID,
        StreamWireMode::Fast { scales },
        stream,
    )?;
    pipeline
        .stream_mut()
        .capture(Duration::from_millis(args.duration_ms))?;

    println!("opening {} @ {}", args.port, args.baud);
    println!(
        "FAST Plot channels: {}",
        DEFAULT_CHANNELS.join(", ")
    );
    println!("capture: {} ms @ {} Hz", args.duration_ms, FAST_SAMPLE_RATE_HZ);

    let session = DeviceSession::open_usb(&args.port, args.baud)?;
    let events = session.subscribe()?;

    session.plot_config(PLOT_GROUP_FAST, CONFIG_ID, &ids)?;
    session.plot_start(PLOT_FAST_MASK)?;

    let deadline = Instant::now() + Duration::from_millis(args.duration_ms + 2_000);
    let mut frames = 0usize;
    let mut samples_received = 0usize;
    let mut last_lost_total = 0u64;

    while pipeline.stream().state() != nmixx_app::StreamState::Paused {
        let now = Instant::now();
        if now >= deadline {
            let _ = session.plot_stop(PLOT_FAST_MASK);
            return Err(format!(
                "FAST Plot capture timed out after {} ms",
                args.duration_ms + 2_000
            )
            .into());
        }

        match events.recv_timeout(deadline.saturating_duration_since(now)) {
            Ok(SessionEvent::FastData(frame)) => {
                let report = pipeline.ingest_fast(&frame)?;
                frames += 1;
                samples_received += report.samples_received;
                last_lost_total = report.lost_frames_total;
            }
            Ok(_) => {}
            Err(error) => {
                let _ = session.plot_stop(PLOT_FAST_MASK);
                return Err(format!("waiting for FAST Plot data failed: {error}").into());
            }
        }
    }

    session.plot_stop(PLOT_FAST_MASK)?;

    let snapshot = pipeline.snapshot();
    println!("frames received: {frames}");
    println!("samples received: {samples_received}");
    println!("samples stored: {}", snapshot.sample_count());
    println!("sequence lost frames: {last_lost_total}");

    for channel in 0..ids.len() {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for sample in 0..snapshot.sample_count() {
            let value = snapshot
                .sample(sample)
                .expect("sample index already bounded")[channel];
            min = min.min(value);
            max = max.max(value);
        }
        println!(
            "{} (0x{:04X}): min={:.6} max={:.6}",
            DEFAULT_CHANNELS[channel], ids[channel], min, max
        );
    }

    if snapshot.sample_count() == 0 {
        return Err("no FAST Plot samples were captured".into());
    }

    println!("PASS: Plot CONFIG/START/FAST DATA/STOP and RAM capture are working");
    Ok(())
}
