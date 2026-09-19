use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use thiserror::Error;

use crate::{
    DevicePlotCapabilities, DeviceSession, HostSchema, PLOT_FAST_MASK, PLOT_GROUP_FAST, SessionError,
    SessionEvent, StreamConfig, StreamError, StreamPipeline, StreamPipelineError, StreamSession,
    StreamSnapshot, StreamState, StreamWireMode,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ScopeChannel {
    pub id: u16,
    pub label: String,
    pub unit: Option<String>,
    pub scale: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScopeConfig {
    pub config_id: u8,
    pub sample_rate_hz: u32,
    pub history: Duration,
    pub channels: Vec<ScopeChannel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeStatus {
    pub state: StreamState,
    pub samples: usize,
    pub capacity_samples: usize,
    pub lost_frames: u64,
}

#[derive(Debug, Error)]
pub enum ScopeError {
    #[error("Scope requires at least one FAST channel")]
    EmptyChannels,
    #[error("Scope selected {actual} FAST channels but device supports at most {max}")]
    TooManyChannels { actual: usize, max: usize },
    #[error("parameter 0x{0:04X} is not reported by device Plot capabilities")]
    UnknownChannel(u16),
    #[error("parameter 0x{0:04X} is not FAST-capable on this device")]
    NotFastCapable(u16),
    #[error("Scope channel {0} has invalid FAST plot scale")]
    InvalidScale(String),
    #[error("Scope runtime failed: {0}")]
    Runtime(String),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Stream(#[from] StreamError),
    #[error(transparent)]
    Pipeline(#[from] StreamPipelineError),
    #[error("Scope worker is closed")]
    Closed,
}

struct SharedState {
    pipeline: StreamPipeline,
    lost_frames: u64,
    runtime_error: Option<String>,
}

enum ScopeCommand {
    Shutdown,
}

pub struct ScopeSession {
    session: DeviceSession,
    config: ScopeConfig,
    shared: Arc<Mutex<SharedState>>,
    command_tx: mpsc::Sender<ScopeCommand>,
    worker: Option<JoinHandle<()>>,
}

impl ScopeSession {
    pub fn from_fast_capabilities(
        session: DeviceSession,
        capabilities: &DevicePlotCapabilities,
        schema: &HostSchema,
        parameter_ids: &[u16],
        history: Duration,
        config_id: u8,
    ) -> Result<Self, ScopeError> {
        if parameter_ids.is_empty() {
            return Err(ScopeError::EmptyChannels);
        }
        if parameter_ids.len() > capabilities.fast_max_channels as usize {
            return Err(ScopeError::TooManyChannels {
                actual: parameter_ids.len(),
                max: capabilities.fast_max_channels as usize,
            });
        }

        let mut channels = Vec::with_capacity(parameter_ids.len());
        for id in parameter_ids {
            let capability = capabilities.channel(*id).ok_or(ScopeError::UnknownChannel(*id))?;
            if !capability.supports_fast() {
                return Err(ScopeError::NotFastCapable(*id));
            }
            let metadata = schema.parameter_by_id(*id);
            channels.push(ScopeChannel {
                id: *id,
                label: metadata
                    .map(|value| value.label.clone())
                    .unwrap_or_else(|| format!("0x{id:04X}")),
                unit: metadata.and_then(|value| value.unit.clone()),
                scale: capability.fast_scale,
            });
        }

        Self::new(
            session,
            ScopeConfig {
                config_id,
                sample_rate_hz: capabilities.fast_rate_hz,
                history,
                channels,
            },
        )
    }

    pub fn new(session: DeviceSession, config: ScopeConfig) -> Result<Self, ScopeError> {
        if config.channels.is_empty() {
            return Err(ScopeError::EmptyChannels);
        }
        for channel in &config.channels {
            if !channel.scale.is_finite() || channel.scale <= 0.0 {
                return Err(ScopeError::InvalidScale(channel.label.clone()));
            }
        }

        let ids: Vec<u16> = config.channels.iter().map(|channel| channel.id).collect();
        session.plot_config(PLOT_GROUP_FAST, config.config_id, &ids)?;

        let stream = StreamSession::new(StreamConfig {
            sample_rate_hz: config.sample_rate_hz,
            channel_count: config.channels.len(),
            history: config.history,
        })?;
        let pipeline = StreamPipeline::new(
            config.config_id,
            StreamWireMode::Fast {
                scales: config.channels.iter().map(|channel| channel.scale).collect(),
            },
            stream,
        )?;

        let shared = Arc::new(Mutex::new(SharedState {
            pipeline,
            lost_frames: 0,
            runtime_error: None,
        }));
        let events = session.subscribe()?;
        let (command_tx, command_rx) = mpsc::channel();
        let worker_shared = Arc::clone(&shared);
        let worker_session = session.clone();
        let worker = thread::Builder::new()
            .name("nmixx-scope".into())
            .spawn(move || scope_worker(worker_session, events, command_rx, worker_shared))
            .map_err(|_| ScopeError::Closed)?;

        Ok(Self { session, config, shared, command_tx, worker: Some(worker) })
    }

    pub fn config(&self) -> &ScopeConfig {
        &self.config
    }

    pub fn live(&self) -> Result<(), ScopeError> {
        let already_live = {
            let mut shared = self.shared.lock().map_err(|_| ScopeError::Closed)?;
            ensure_runtime_ok(&shared)?;
            let already_live = shared.pipeline.stream().state() == StreamState::Live;
            if !already_live {
                shared.pipeline.reset_sequence();
                shared.lost_frames = 0;
                shared.pipeline.stream_mut().live();
            }
            already_live
        };
        if already_live {
            return Ok(());
        }
        if let Err(error) = self.session.plot_start(PLOT_FAST_MASK) {
            if let Ok(mut shared) = self.shared.lock() {
                shared.pipeline.stream_mut().pause();
            }
            return Err(error.into());
        }
        Ok(())
    }

    pub fn resume(&self) -> Result<(), ScopeError> {
        self.live()
    }

    pub fn pause(&self) -> Result<(), ScopeError> {
        let state = self.shared.lock().map_err(|_| ScopeError::Closed)?.pipeline.stream().state();
        if matches!(state, StreamState::Live | StreamState::Capturing) {
            self.session.plot_stop(PLOT_FAST_MASK)?;
        }
        let mut shared = self.shared.lock().map_err(|_| ScopeError::Closed)?;
        shared.pipeline.stream_mut().pause();
        Ok(())
    }

    pub fn stop(&self) -> Result<(), ScopeError> {
        let state = self.shared.lock().map_err(|_| ScopeError::Closed)?.pipeline.stream().state();
        if matches!(state, StreamState::Live | StreamState::Capturing) {
            self.session.plot_stop(PLOT_FAST_MASK)?;
        }
        let mut shared = self.shared.lock().map_err(|_| ScopeError::Closed)?;
        shared.pipeline.stream_mut().stop();
        Ok(())
    }

    pub fn clear(&self) -> Result<(), ScopeError> {
        let mut shared = self.shared.lock().map_err(|_| ScopeError::Closed)?;
        ensure_runtime_ok(&shared)?;
        shared.pipeline.stream_mut().clear();
        Ok(())
    }

    pub fn capture(&self, duration: Duration) -> Result<(), ScopeError> {
        let was_running = {
            let mut shared = self.shared.lock().map_err(|_| ScopeError::Closed)?;
            ensure_runtime_ok(&shared)?;
            let was_running = matches!(shared.pipeline.stream().state(), StreamState::Live | StreamState::Capturing);
            shared.pipeline.reset_sequence();
            shared.lost_frames = 0;
            shared.pipeline.stream_mut().capture(duration)?;
            was_running
        };
        if was_running {
            return Ok(());
        }
        if let Err(error) = self.session.plot_start(PLOT_FAST_MASK) {
            if let Ok(mut shared) = self.shared.lock() {
                shared.pipeline.stream_mut().pause();
            }
            return Err(error.into());
        }
        Ok(())
    }

    pub fn status(&self) -> Result<ScopeStatus, ScopeError> {
        let shared = self.shared.lock().map_err(|_| ScopeError::Closed)?;
        ensure_runtime_ok(&shared)?;
        Ok(ScopeStatus {
            state: shared.pipeline.stream().state(),
            samples: shared.pipeline.stream().len(),
            capacity_samples: shared.pipeline.stream().capacity_samples(),
            lost_frames: shared.lost_frames,
        })
    }

    pub fn snapshot(&self) -> Result<StreamSnapshot, ScopeError> {
        let shared = self.shared.lock().map_err(|_| ScopeError::Closed)?;
        ensure_runtime_ok(&shared)?;
        Ok(shared.pipeline.snapshot())
    }

    pub fn snapshot_tail(&self, max_samples: usize) -> Result<StreamSnapshot, ScopeError> {
        let shared = self.shared.lock().map_err(|_| ScopeError::Closed)?;
        ensure_runtime_ok(&shared)?;
        Ok(shared.pipeline.snapshot_tail(max_samples))
    }
}

impl Drop for ScopeSession {
    fn drop(&mut self) {
        let state = self.shared.lock().ok().map(|shared| shared.pipeline.stream().state());
        if matches!(state, Some(StreamState::Live | StreamState::Capturing)) {
            let _ = self.session.plot_stop(PLOT_FAST_MASK);
        }
        let _ = self.command_tx.send(ScopeCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn ensure_runtime_ok(shared: &SharedState) -> Result<(), ScopeError> {
    match &shared.runtime_error {
        Some(error) => Err(ScopeError::Runtime(error.clone())),
        None => Ok(()),
    }
}

fn scope_worker(
    session: DeviceSession,
    events: mpsc::Receiver<SessionEvent>,
    commands: mpsc::Receiver<ScopeCommand>,
    shared: Arc<Mutex<SharedState>>,
) {
    loop {
        if matches!(commands.try_recv(), Ok(ScopeCommand::Shutdown) | Err(mpsc::TryRecvError::Disconnected)) {
            break;
        }

        match events.recv_timeout(Duration::from_millis(20)) {
            Ok(SessionEvent::FastData(frame)) => {
                let (stop_plot, fatal) = {
                    let Ok(mut state) = shared.lock() else { break; };
                    match state.pipeline.ingest_fast(&frame) {
                        Ok(report) => {
                            state.lost_frames = report.lost_frames_total;
                            (
                                state.pipeline.stream().state() == StreamState::Paused
                                    && report.samples_stored > 0,
                                false,
                            )
                        }
                        Err(error) => {
                            state.runtime_error = Some(error.to_string());
                            state.pipeline.stream_mut().pause();
                            (true, true)
                        }
                    }
                };
                if stop_plot {
                    let _ = session.plot_stop(PLOT_FAST_MASK);
                }
                if fatal {
                    break;
                }
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}
