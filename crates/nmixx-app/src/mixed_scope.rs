use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use thiserror::Error;

use crate::{
    DevicePlotCapabilities, DeviceSession, HostSchema, PLOT_FAST_MASK, PLOT_GROUP_FAST,
    PLOT_GROUP_NORMAL, PLOT_NORMAL_MASK, SessionError, SessionEvent, StreamConfig, StreamError,
    StreamPipeline, StreamPipelineError, StreamSession, StreamState, StreamWireMode,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeRate {
    Fast,
    Normal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeSelection {
    pub id: u16,
    pub rate: ScopeRate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MixedScopeChannel {
    pub id: u16,
    pub symbol: String,
    pub unit: Option<String>,
    pub rate: ScopeRate,
    pub sample_rate_hz: u32,
    pub scale: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MixedScopeConfig {
    pub config_id: u8,
    pub history: Duration,
    pub channels: Vec<MixedScopeChannel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MixedScopeStatus {
    pub state: StreamState,
    pub samples: usize,
    pub capacity_samples: usize,
    pub lost_frames: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MixedScopeSeries {
    pub id: u16,
    pub sample_rate_hz: u32,
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MixedScopeSnapshot {
    pub state: StreamState,
    pub lost_frames: u64,
    pub series: Vec<MixedScopeSeries>,
}

#[derive(Debug, Error)]
pub enum MixedScopeError {
    #[error("Scope requires at least one channel")]
    EmptyChannels,
    #[error("Scope selected {actual} FAST channels but device supports at most {max}")]
    TooManyFastChannels { actual: usize, max: usize },
    #[error("Scope selected {actual} NORMAL channels but device supports at most {max}")]
    TooManyNormalChannels { actual: usize, max: usize },
    #[error("parameter 0x{0:04X} is not reported by device Plot capabilities")]
    UnknownChannel(u16),
    #[error("parameter 0x{0:04X} is not FAST-capable on this device")]
    NotFastCapable(u16),
    #[error("parameter 0x{0:04X} is not NORMAL-capable on this device")]
    NotNormalCapable(u16),
    #[error("FAST channel 0x{0:04X} has invalid plot scale")]
    InvalidFastScale(u16),
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

struct GroupRuntime {
    ids: Vec<u16>,
    pipeline: StreamPipeline,
    lost_frames: u64,
}

struct SharedState {
    fast: Option<GroupRuntime>,
    normal: Option<GroupRuntime>,
    runtime_error: Option<String>,
}

enum ScopeCommand {
    Shutdown,
}

pub struct MixedScopeSession {
    session: DeviceSession,
    config: MixedScopeConfig,
    group_mask: u8,
    shared: Arc<Mutex<SharedState>>,
    command_tx: mpsc::Sender<ScopeCommand>,
    worker: Option<JoinHandle<()>>,
}

impl MixedScopeSession {
    pub fn from_capabilities(
        session: DeviceSession,
        capabilities: &DevicePlotCapabilities,
        schema: &HostSchema,
        selections: &[ScopeSelection],
        history: Duration,
        config_id: u8,
    ) -> Result<Self, MixedScopeError> {
        if selections.is_empty() {
            return Err(MixedScopeError::EmptyChannels);
        }

        let fast_count = selections.iter().filter(|item| item.rate == ScopeRate::Fast).count();
        let normal_count = selections.iter().filter(|item| item.rate == ScopeRate::Normal).count();
        if fast_count > capabilities.fast_max_channels as usize {
            return Err(MixedScopeError::TooManyFastChannels {
                actual: fast_count,
                max: capabilities.fast_max_channels as usize,
            });
        }
        if normal_count > capabilities.normal_max_channels as usize {
            return Err(MixedScopeError::TooManyNormalChannels {
                actual: normal_count,
                max: capabilities.normal_max_channels as usize,
            });
        }

        let mut channels = Vec::with_capacity(selections.len());
        for selection in selections {
            let capability = capabilities
                .channel(selection.id)
                .ok_or(MixedScopeError::UnknownChannel(selection.id))?;
            match selection.rate {
                ScopeRate::Fast if !capability.supports_fast() => {
                    return Err(MixedScopeError::NotFastCapable(selection.id));
                }
                ScopeRate::Normal if !capability.supports_normal() => {
                    return Err(MixedScopeError::NotNormalCapable(selection.id));
                }
                _ => {}
            }

            if selection.rate == ScopeRate::Fast
                && (!capability.fast_scale.is_finite() || capability.fast_scale <= 0.0)
            {
                return Err(MixedScopeError::InvalidFastScale(selection.id));
            }

            let metadata = schema.parameter_by_id(selection.id);
            channels.push(MixedScopeChannel {
                id: selection.id,
                symbol: metadata
                    .map(|value| value.symbol.clone())
                    .unwrap_or_else(|| format!("0x{:04X}", selection.id)),
                unit: metadata.and_then(|value| value.unit.clone()),
                rate: selection.rate,
                sample_rate_hz: match selection.rate {
                    ScopeRate::Fast => capabilities.fast_rate_hz,
                    ScopeRate::Normal => capabilities.normal_rate_hz,
                },
                scale: if selection.rate == ScopeRate::Fast {
                    capability.fast_scale
                } else {
                    1.0
                },
            });
        }

        let config = MixedScopeConfig {
            config_id,
            history,
            channels,
        };
        Self::new(session, config)
    }

    pub fn new(session: DeviceSession, config: MixedScopeConfig) -> Result<Self, MixedScopeError> {
        let fast_channels: Vec<&MixedScopeChannel> = config
            .channels
            .iter()
            .filter(|channel| channel.rate == ScopeRate::Fast)
            .collect();
        let normal_channels: Vec<&MixedScopeChannel> = config
            .channels
            .iter()
            .filter(|channel| channel.rate == ScopeRate::Normal)
            .collect();

        let mut group_mask = 0u8;
        let fast = if fast_channels.is_empty() {
            None
        } else {
            let ids: Vec<u16> = fast_channels.iter().map(|channel| channel.id).collect();
            session.plot_config(PLOT_GROUP_FAST, config.config_id, &ids)?;
            let stream = StreamSession::new(StreamConfig {
                sample_rate_hz: fast_channels[0].sample_rate_hz,
                channel_count: ids.len(),
                history: config.history,
            })?;
            let pipeline = StreamPipeline::new(
                config.config_id,
                StreamWireMode::Fast {
                    scales: fast_channels.iter().map(|channel| channel.scale).collect(),
                },
                stream,
            )?;
            group_mask |= PLOT_FAST_MASK;
            Some(GroupRuntime {
                ids,
                pipeline,
                lost_frames: 0,
            })
        };

        let normal = if normal_channels.is_empty() {
            None
        } else {
            let ids: Vec<u16> = normal_channels.iter().map(|channel| channel.id).collect();
            session.plot_config(PLOT_GROUP_NORMAL, config.config_id, &ids)?;
            let stream = StreamSession::new(StreamConfig {
                sample_rate_hz: normal_channels[0].sample_rate_hz,
                channel_count: ids.len(),
                history: config.history,
            })?;
            let pipeline = StreamPipeline::new(config.config_id, StreamWireMode::Normal, stream)?;
            group_mask |= PLOT_NORMAL_MASK;
            Some(GroupRuntime {
                ids,
                pipeline,
                lost_frames: 0,
            })
        };

        if group_mask == 0 {
            return Err(MixedScopeError::EmptyChannels);
        }

        let shared = Arc::new(Mutex::new(SharedState {
            fast,
            normal,
            runtime_error: None,
        }));
        let events = session.subscribe()?;
        let (command_tx, command_rx) = mpsc::channel();
        let worker_shared = Arc::clone(&shared);
        let worker_session = session.clone();
        let worker = thread::Builder::new()
            .name("nmixx-mixed-scope".into())
            .spawn(move || scope_worker(worker_session, events, command_rx, worker_shared))
            .map_err(|_| MixedScopeError::Closed)?;

        Ok(Self {
            session,
            config,
            group_mask,
            shared,
            command_tx,
            worker: Some(worker),
        })
    }

    pub fn config(&self) -> &MixedScopeConfig {
        &self.config
    }

    pub fn live(&self) -> Result<(), MixedScopeError> {
        let already_live = {
            let mut shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
            ensure_runtime_ok(&shared)?;
            let already_live = group_states(&shared)
                .all(|state| state == StreamState::Live);
            if !already_live {
                for_each_group_mut(&mut shared, |group| {
                    group.pipeline.reset_sequence();
                    group.lost_frames = 0;
                    group.pipeline.stream_mut().live();
                });
            }
            already_live
        };
        if !already_live {
            if let Err(error) = self.session.plot_start(self.group_mask) {
                if let Ok(mut shared) = self.shared.lock() {
                    for_each_group_mut(&mut shared, |group| group.pipeline.stream_mut().pause());
                }
                return Err(error.into());
            }
        }
        Ok(())
    }

    pub fn resume(&self) -> Result<(), MixedScopeError> {
        self.live()
    }

    pub fn pause(&self) -> Result<(), MixedScopeError> {
        if self.is_running()? {
            self.session.plot_stop(self.group_mask)?;
        }
        let mut shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
        for_each_group_mut(&mut shared, |group| group.pipeline.stream_mut().pause());
        Ok(())
    }

    pub fn stop(&self) -> Result<(), MixedScopeError> {
        if self.is_running()? {
            self.session.plot_stop(self.group_mask)?;
        }
        let mut shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
        for_each_group_mut(&mut shared, |group| group.pipeline.stream_mut().stop());
        Ok(())
    }

    pub fn clear(&self) -> Result<(), MixedScopeError> {
        let mut shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
        ensure_runtime_ok(&shared)?;
        for_each_group_mut(&mut shared, |group| group.pipeline.stream_mut().clear());
        Ok(())
    }

    pub fn capture(&self, duration: Duration) -> Result<(), MixedScopeError> {
        let was_running = self.is_running()?;
        {
            let mut shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
            ensure_runtime_ok(&shared)?;
            if let Some(group) = shared.fast.as_mut() {
                group.pipeline.reset_sequence();
                group.lost_frames = 0;
                group.pipeline.stream_mut().capture(duration)?;
            }
            if let Some(group) = shared.normal.as_mut() {
                group.pipeline.reset_sequence();
                group.lost_frames = 0;
                group.pipeline.stream_mut().capture(duration)?;
            }
        }
        if !was_running {
            self.session.plot_start(self.group_mask)?;
        }
        Ok(())
    }

    pub fn status(&self) -> Result<MixedScopeStatus, MixedScopeError> {
        let shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
        ensure_runtime_ok(&shared)?;
        let state = combined_state(&shared);
        let samples = groups(&shared)
            .map(|group| group.pipeline.stream().len())
            .sum();
        let capacity_samples = groups(&shared)
            .map(|group| group.pipeline.stream().capacity_samples())
            .sum();
        let lost_frames = groups(&shared).map(|group| group.lost_frames).sum();
        Ok(MixedScopeStatus {
            state,
            samples,
            capacity_samples,
            lost_frames,
        })
    }

    pub fn snapshot_tail(&self, window: Duration) -> Result<MixedScopeSnapshot, MixedScopeError> {
        self.snapshot_window(window, Duration::ZERO)
    }

    pub fn snapshot_window(
        &self,
        window: Duration,
        end_offset: Duration,
    ) -> Result<MixedScopeSnapshot, MixedScopeError> {
        let shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
        ensure_runtime_ok(&shared)?;

        let mut series = Vec::with_capacity(self.config.channels.len());
        for channel in &self.config.channels {
            let group = match channel.rate {
                ScopeRate::Fast => shared.fast.as_ref(),
                ScopeRate::Normal => shared.normal.as_ref(),
            }
            .ok_or(MixedScopeError::Closed)?;

            let index = group
                .ids
                .iter()
                .position(|id| *id == channel.id)
                .ok_or(MixedScopeError::UnknownChannel(channel.id))?;

            let rate = f64::from(channel.sample_rate_hz);
            let wanted = (window.as_secs_f64() * rate).ceil() as usize;
            let offset = (end_offset.as_secs_f64() * rate).round() as usize;
            let snapshot = group.pipeline.snapshot();

            let available = snapshot.sample_count();
            let end = available.saturating_sub(offset.min(available));
            let start = end.saturating_sub(wanted.max(1));

            let mut values = Vec::with_capacity(end.saturating_sub(start));
            for sample_index in start..end {
                if let Some(sample) = snapshot.sample(sample_index) {
                    values.push(sample[index]);
                }
            }
            series.push(MixedScopeSeries {
                id: channel.id,
                sample_rate_hz: channel.sample_rate_hz,
                values,
            });
        }

        Ok(MixedScopeSnapshot {
            state: combined_state(&shared),
            lost_frames: groups(&shared).map(|group| group.lost_frames).sum(),
            series,
        })
    }

    fn is_running(&self) -> Result<bool, MixedScopeError> {
        let shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
        ensure_runtime_ok(&shared)?;
        Ok(group_states(&shared).any(|state| {
            matches!(state, StreamState::Live | StreamState::Capturing)
        }))
    }
}

impl Drop for MixedScopeSession {
    fn drop(&mut self) {
        if self.is_running().unwrap_or(false) {
            let _ = self.session.plot_stop(self.group_mask);
        }
        let _ = self.command_tx.send(ScopeCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn ensure_runtime_ok(shared: &SharedState) -> Result<(), MixedScopeError> {
    match &shared.runtime_error {
        Some(error) => Err(MixedScopeError::Runtime(error.clone())),
        None => Ok(()),
    }
}

fn groups(shared: &SharedState) -> impl Iterator<Item = &GroupRuntime> {
    shared.fast.iter().chain(shared.normal.iter())
}


fn group_states(shared: &SharedState) -> impl Iterator<Item = StreamState> + '_ {
    groups(shared).map(|group| group.pipeline.stream().state())
}

fn for_each_group_mut(shared: &mut SharedState, mut call: impl FnMut(&mut GroupRuntime)) {
    if let Some(group) = shared.fast.as_mut() {
        call(group);
    }
    if let Some(group) = shared.normal.as_mut() {
        call(group);
    }
}

fn combined_state(shared: &SharedState) -> StreamState {
    let states: Vec<StreamState> = group_states(shared).collect();
    if states.iter().any(|state| *state == StreamState::Capturing) {
        StreamState::Capturing
    } else if states.iter().any(|state| *state == StreamState::Live) {
        StreamState::Live
    } else if states.iter().any(|state| *state == StreamState::Paused) {
        StreamState::Paused
    } else {
        StreamState::Stopped
    }
}

fn scope_worker(
    session: DeviceSession,
    events: mpsc::Receiver<SessionEvent>,
    commands: mpsc::Receiver<ScopeCommand>,
    shared: Arc<Mutex<SharedState>>,
) {
    loop {
        if matches!(
            commands.try_recv(),
            Ok(ScopeCommand::Shutdown) | Err(mpsc::TryRecvError::Disconnected)
        ) {
            break;
        }

        match events.recv_timeout(Duration::from_millis(20)) {
            Ok(SessionEvent::FastData(frame)) => {
                if !ingest_group(&session, &shared, ScopeRate::Fast, &frame) {
                    break;
                }
            }
            Ok(SessionEvent::NormalData(frame)) => {
                if !ingest_group(&session, &shared, ScopeRate::Normal, &frame) {
                    break;
                }
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn ingest_group(
    session: &DeviceSession,
    shared: &Arc<Mutex<SharedState>>,
    rate: ScopeRate,
    frame: &nmixx_core::wire::CanFdFrame,
) -> bool {
    let stop_mask = match rate {
        ScopeRate::Fast => PLOT_FAST_MASK,
        ScopeRate::Normal => PLOT_NORMAL_MASK,
    };

    let (stop_plot, fatal) = {
        let Ok(mut state) = shared.lock() else { return false; };
        let group = match rate {
            ScopeRate::Fast => state.fast.as_mut(),
            ScopeRate::Normal => state.normal.as_mut(),
        };
        let Some(group) = group else { return true; };

        let result = match rate {
            ScopeRate::Fast => group.pipeline.ingest_fast(frame),
            ScopeRate::Normal => group.pipeline.ingest_normal(frame),
        };
        match result {
            Ok(report) => {
                group.lost_frames = report.lost_frames_total;
                (
                    group.pipeline.stream().state() == StreamState::Paused
                        && report.samples_stored > 0,
                    false,
                )
            }
            Err(error) => {
                let message = error.to_string();
                group.pipeline.stream_mut().pause();
                state.runtime_error = Some(message);
                (true, true)
            }
        }
    };

    if stop_plot {
        let _ = session.plot_stop(stop_mask);
    }
    !fatal
}
