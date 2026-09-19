use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nmixx_core::protocol::{
    SequenceTracker, StreamDecodeError, decode_fast_data, decode_normal_data,
};
use thiserror::Error;

use crate::{
    DevicePlotCapabilities, DeviceSession, HostSchema, PLOT_FAST_MASK, PLOT_GROUP_FAST,
    PLOT_GROUP_NORMAL, PLOT_NORMAL_MASK, SessionError, SessionEvent, StreamConfig, StreamError,
    StreamSession, StreamState,
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
    pub label: String,
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
    #[error("Scope group already has a pending hot reconfiguration")]
    ReconfigurePending,
    #[error("Scope received unknown Config_ID {0}")]
    UnknownConfig(u8),
    #[error("Scope runtime failed: {0}")]
    Runtime(String),
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error(transparent)]
    Stream(#[from] StreamError),
    #[error(transparent)]
    Decode(#[from] StreamDecodeError),
    #[error("Scope worker is closed")]
    Closed,
}

#[derive(Clone)]
struct GroupLayout {
    config_id: u8,
    ids: Vec<u16>,
    scales: Vec<f32>,
}

struct GroupRuntime {
    active: GroupLayout,
    pending: Option<GroupLayout>,
    sequence: SequenceTracker,
    lost_frames: u64,
}

struct ChannelHistory {
    rate: ScopeRate,
    stream: StreamSession,
}

struct SharedState {
    fast: Option<GroupRuntime>,
    normal: Option<GroupRuntime>,
    histories: HashMap<u16, ChannelHistory>,
    state: StreamState,
    runtime_error: Option<String>,
}

enum ScopeCommand {
    Shutdown,
}

pub struct MixedScopeSession {
    session: DeviceSession,
    capabilities: DevicePlotCapabilities,
    schema: HostSchema,
    config: Mutex<MixedScopeConfig>,
    shared: Arc<Mutex<SharedState>>,
    next_config_id: Mutex<u8>,
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
        let config = build_config(capabilities, schema, selections, history, config_id)?;
        Self::new_with_context(session, capabilities.clone(), schema.clone(), config)
    }

    fn new_with_context(
        session: DeviceSession,
        capabilities: DevicePlotCapabilities,
        schema: HostSchema,
        config: MixedScopeConfig,
    ) -> Result<Self, MixedScopeError> {
        let fast_layout = group_layout(&config, ScopeRate::Fast);
        let normal_layout = group_layout(&config, ScopeRate::Normal);

        if let Some(layout) = &fast_layout {
            session.plot_config(PLOT_GROUP_FAST, layout.config_id, &layout.ids)?;
        }
        if let Some(layout) = &normal_layout {
            session.plot_config(PLOT_GROUP_NORMAL, layout.config_id, &layout.ids)?;
        }

        let mut histories = HashMap::new();
        for channel in &config.channels {
            histories.insert(
                channel.id,
                ChannelHistory {
                    rate: channel.rate,
                    stream: StreamSession::new(StreamConfig {
                        sample_rate_hz: channel.sample_rate_hz,
                        channel_count: 1,
                        history: config.history,
                    })?,
                },
            );
        }

        let shared = Arc::new(Mutex::new(SharedState {
            fast: fast_layout.map(group_runtime),
            normal: normal_layout.map(group_runtime),
            histories,
            state: StreamState::Stopped,
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

        let next_config_id = if config.config_id == u8::MAX {
            1
        } else {
            config.config_id + 1
        };

        Ok(Self {
            session,
            capabilities,
            schema,
            config: Mutex::new(config),
            shared,
            next_config_id: Mutex::new(next_config_id),
            command_tx,
            worker: Some(worker),
        })
    }

    pub fn config(&self) -> Result<MixedScopeConfig, MixedScopeError> {
        self.config
            .lock()
            .map_err(|_| MixedScopeError::Closed)
            .map(|config| config.clone())
    }

    pub fn reconfigure(&self, selections: &[ScopeSelection]) -> Result<MixedScopeConfig, MixedScopeError> {
        let old_config = self.config()?;
        let mut new_config = build_config(
            &self.capabilities,
            &self.schema,
            selections,
            old_config.history,
            old_config.config_id,
        )?;

        let new_fast_ids = ids_for_rate(&new_config, ScopeRate::Fast);
        let new_normal_ids = ids_for_rate(&new_config, ScopeRate::Normal);

        let (state, old_fast_ids, old_normal_ids, fast_pending, normal_pending) = {
            let shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
            ensure_runtime_ok(&shared)?;
            (
                shared.state,
                shared.fast.as_ref().map(|group| group.active.ids.clone()).unwrap_or_default(),
                shared.normal.as_ref().map(|group| group.active.ids.clone()).unwrap_or_default(),
                shared.fast.as_ref().is_some_and(|group| group.pending.is_some()),
                shared.normal.as_ref().is_some_and(|group| group.pending.is_some()),
            )
        };

        let fast_changed = old_fast_ids != new_fast_ids;
        let normal_changed = old_normal_ids != new_normal_ids;

        if (fast_changed && fast_pending) || (normal_changed && normal_pending) {
            return Err(MixedScopeError::ReconfigurePending);
        }

        prepare_histories(&self.shared, &new_config)?;

        if fast_changed {
            self.reconfigure_group(ScopeRate::Fast, &new_config, state)?;
        }
        if normal_changed {
            self.reconfigure_group(ScopeRate::Normal, &new_config, state)?;
        }

        new_config.config_id = self.current_config_marker()?;
        *self.config.lock().map_err(|_| MixedScopeError::Closed)? = new_config.clone();
        Ok(new_config)
    }

    fn reconfigure_group(
        &self,
        rate: ScopeRate,
        config: &MixedScopeConfig,
        state: StreamState,
    ) -> Result<(), MixedScopeError> {
        let ids = ids_for_rate(config, rate);
        let mask = rate_mask(rate);
        let group = rate_group(rate);

        if ids.is_empty() {
            let had_group = {
                let shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
                group_ref(&shared, rate).is_some()
            };
            if had_group && matches!(state, StreamState::Live | StreamState::Capturing) {
                self.session.plot_stop(mask)?;
            }
            let mut shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
            *group_mut(&mut shared, rate) = None;
            return Ok(());
        }

        let layout = GroupLayout {
            config_id: self.allocate_config_id()?,
            ids,
            scales: scales_for_rate(config, rate),
        };

        let had_group = {
            let shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
            group_ref(&shared, rate).is_some()
        };

        if had_group && matches!(state, StreamState::Live | StreamState::Capturing) {
            {
                let mut shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
                let runtime = group_mut(&mut shared, rate)
                    .as_mut()
                    .ok_or(MixedScopeError::Closed)?;
                runtime.pending = Some(layout.clone());
            }
            if let Err(error) = self.session.plot_config(group, layout.config_id, &layout.ids) {
                if let Ok(mut shared) = self.shared.lock() {
                    if let Some(runtime) = group_mut(&mut shared, rate).as_mut() {
                        runtime.pending = None;
                    }
                }
                return Err(error.into());
            }
            return Ok(());
        }

        self.session.plot_config(group, layout.config_id, &layout.ids)?;

        {
            let mut shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
            *group_mut(&mut shared, rate) = Some(group_runtime(layout));
        }

        if !had_group && matches!(state, StreamState::Live | StreamState::Capturing) {
            self.session.plot_start(mask)?;
        }

        Ok(())
    }

    fn allocate_config_id(&self) -> Result<u8, MixedScopeError> {
        let mut next = self.next_config_id.lock().map_err(|_| MixedScopeError::Closed)?;
        let value = *next;
        *next = if value == u8::MAX { 1 } else { value + 1 };
        Ok(value)
    }

    fn current_config_marker(&self) -> Result<u8, MixedScopeError> {
        let next = self.next_config_id.lock().map_err(|_| MixedScopeError::Closed)?;
        Ok(next.wrapping_sub(1).max(1))
    }

    pub fn live(&self) -> Result<(), MixedScopeError> {
        let (mask, already_live) = {
            let mut shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
            ensure_runtime_ok(&shared)?;
            let already_live = shared.state == StreamState::Live;
            if !already_live {
                shared.state = StreamState::Live;
                for history in shared.histories.values_mut() {
                    history.stream.live();
                }
            }
            (group_mask(&shared), already_live)
        };

        if !already_live && mask != 0 {
            self.session.plot_start(mask)?;
        }
        Ok(())
    }

    pub fn resume(&self) -> Result<(), MixedScopeError> {
        self.live()
    }

    pub fn pause(&self) -> Result<(), MixedScopeError> {
        let mask = {
            let shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
            ensure_runtime_ok(&shared)?;
            group_mask(&shared)
        };
        if mask != 0 {
            self.session.plot_stop(mask)?;
        }
        let mut shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
        shared.state = StreamState::Paused;
        for history in shared.histories.values_mut() {
            history.stream.pause();
        }
        Ok(())
    }

    pub fn stop(&self) -> Result<(), MixedScopeError> {
        let mask = {
            let shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
            ensure_runtime_ok(&shared)?;
            group_mask(&shared)
        };
        if mask != 0 {
            self.session.plot_stop(mask)?;
        }
        let mut shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
        shared.state = StreamState::Stopped;
        for history in shared.histories.values_mut() {
            history.stream.stop();
        }
        Ok(())
    }

    pub fn clear(&self) -> Result<(), MixedScopeError> {
        let mut shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
        ensure_runtime_ok(&shared)?;
        for history in shared.histories.values_mut() {
            history.stream.clear();
        }
        Ok(())
    }

    pub fn capture(&self, duration: Duration) -> Result<(), MixedScopeError> {
        let mask = {
            let mut shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
            ensure_runtime_ok(&shared)?;
            shared.state = StreamState::Capturing;
            for history in shared.histories.values_mut() {
                history.stream.capture(duration)?;
            }
            group_mask(&shared)
        };
        if mask != 0 {
            self.session.plot_start(mask)?;
        }
        Ok(())
    }

    pub fn status(&self) -> Result<MixedScopeStatus, MixedScopeError> {
        let config = self.config()?;
        let shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
        ensure_runtime_ok(&shared)?;

        let mut samples = 0usize;
        let mut capacity_samples = 0usize;
        for channel in &config.channels {
            if let Some(history) = shared.histories.get(&channel.id) {
                samples = samples.saturating_add(history.stream.len());
                capacity_samples = capacity_samples.saturating_add(history.stream.capacity_samples());
            }
        }

        Ok(MixedScopeStatus {
            state: shared.state,
            samples,
            capacity_samples,
            lost_frames: group_lost_frames(&shared),
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
        let config = self.config()?;
        let shared = self.shared.lock().map_err(|_| MixedScopeError::Closed)?;
        ensure_runtime_ok(&shared)?;

        let mut series = Vec::with_capacity(config.channels.len());
        for channel in &config.channels {
            let history = shared
                .histories
                .get(&channel.id)
                .ok_or(MixedScopeError::UnknownChannel(channel.id))?;

            let rate = f64::from(channel.sample_rate_hz);
            let wanted = (window.as_secs_f64() * rate).ceil() as usize;
            let offset = (end_offset.as_secs_f64() * rate).round() as usize;
            let snapshot = history.stream.snapshot();

            let available = snapshot.sample_count();
            let end = available.saturating_sub(offset.min(available));
            let start = end.saturating_sub(wanted.max(1));
            let mut values = Vec::with_capacity(end.saturating_sub(start));
            for sample_index in start..end {
                if let Some(sample) = snapshot.sample(sample_index) {
                    values.push(sample[0]);
                }
            }

            series.push(MixedScopeSeries {
                id: channel.id,
                sample_rate_hz: channel.sample_rate_hz,
                values,
            });
        }

        Ok(MixedScopeSnapshot {
            state: shared.state,
            lost_frames: group_lost_frames(&shared),
            series,
        })
    }
}

impl Drop for MixedScopeSession {
    fn drop(&mut self) {
        let mask = self
            .shared
            .lock()
            .ok()
            .map(|shared| group_mask(&shared))
            .unwrap_or(0);
        if mask != 0 {
            let _ = self.session.plot_stop(mask);
        }
        let _ = self.command_tx.send(ScopeCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn build_config(
    capabilities: &DevicePlotCapabilities,
    schema: &HostSchema,
    selections: &[ScopeSelection],
    history: Duration,
    config_id: u8,
) -> Result<MixedScopeConfig, MixedScopeError> {
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
            label: metadata
                .map(|value| value.label.clone())
                .unwrap_or_else(|| format!("0x{:04X}", selection.id)),
            unit: metadata.and_then(|value| {
                if value.type_name == "position" {
                    Some("turn".to_owned())
                } else {
                    value.unit.clone()
                }
            }),
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

    Ok(MixedScopeConfig {
        config_id,
        history,
        channels,
    })
}

fn group_layout(config: &MixedScopeConfig, rate: ScopeRate) -> Option<GroupLayout> {
    let ids = ids_for_rate(config, rate);
    if ids.is_empty() {
        return None;
    }
    Some(GroupLayout {
        config_id: config.config_id,
        ids,
        scales: scales_for_rate(config, rate),
    })
}

fn group_runtime(layout: GroupLayout) -> GroupRuntime {
    GroupRuntime {
        active: layout,
        pending: None,
        sequence: SequenceTracker::default(),
        lost_frames: 0,
    }
}

fn ids_for_rate(config: &MixedScopeConfig, rate: ScopeRate) -> Vec<u16> {
    config
        .channels
        .iter()
        .filter(|channel| channel.rate == rate)
        .map(|channel| channel.id)
        .collect()
}

fn scales_for_rate(config: &MixedScopeConfig, rate: ScopeRate) -> Vec<f32> {
    config
        .channels
        .iter()
        .filter(|channel| channel.rate == rate)
        .map(|channel| channel.scale)
        .collect()
}

fn rate_group(rate: ScopeRate) -> u8 {
    match rate {
        ScopeRate::Fast => PLOT_GROUP_FAST,
        ScopeRate::Normal => PLOT_GROUP_NORMAL,
    }
}

fn rate_mask(rate: ScopeRate) -> u8 {
    match rate {
        ScopeRate::Fast => PLOT_FAST_MASK,
        ScopeRate::Normal => PLOT_NORMAL_MASK,
    }
}

fn group_ref(shared: &SharedState, rate: ScopeRate) -> Option<&GroupRuntime> {
    match rate {
        ScopeRate::Fast => shared.fast.as_ref(),
        ScopeRate::Normal => shared.normal.as_ref(),
    }
}

fn group_mut(shared: &mut SharedState, rate: ScopeRate) -> &mut Option<GroupRuntime> {
    match rate {
        ScopeRate::Fast => &mut shared.fast,
        ScopeRate::Normal => &mut shared.normal,
    }
}

fn group_mask(shared: &SharedState) -> u8 {
    let mut mask = 0u8;
    if shared.fast.is_some() {
        mask |= PLOT_FAST_MASK;
    }
    if shared.normal.is_some() {
        mask |= PLOT_NORMAL_MASK;
    }
    mask
}

fn group_lost_frames(shared: &SharedState) -> u64 {
    shared.fast.as_ref().map(|group| group.lost_frames).unwrap_or(0)
        + shared.normal.as_ref().map(|group| group.lost_frames).unwrap_or(0)
}

fn prepare_histories(
    shared: &Arc<Mutex<SharedState>>,
    config: &MixedScopeConfig,
) -> Result<(), MixedScopeError> {
    let mut state = shared.lock().map_err(|_| MixedScopeError::Closed)?;
    let active_state = state.state;

    let selected: HashMap<u16, ScopeRate> = config
        .channels
        .iter()
        .map(|channel| (channel.id, channel.rate))
        .collect();

    state.histories.retain(|id, history| {
        selected.get(id).is_some_and(|rate| *rate == history.rate)
    });

    for channel in &config.channels {
        if state.histories.contains_key(&channel.id) {
            continue;
        }

        let mut stream = StreamSession::new(StreamConfig {
            sample_rate_hz: channel.sample_rate_hz,
            channel_count: 1,
            history: config.history,
        })?;
        match active_state {
            StreamState::Live => stream.live(),
            StreamState::Capturing => stream.live(),
            StreamState::Paused => stream.pause(),
            StreamState::Stopped => stream.stop(),
        }

        state.histories.insert(
            channel.id,
            ChannelHistory {
                rate: channel.rate,
                stream,
            },
        );
    }
    Ok(())
}

fn ensure_runtime_ok(shared: &SharedState) -> Result<(), MixedScopeError> {
    match &shared.runtime_error {
        Some(error) => Err(MixedScopeError::Runtime(error.clone())),
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
        if matches!(
            commands.try_recv(),
            Ok(ScopeCommand::Shutdown) | Err(mpsc::TryRecvError::Disconnected)
        ) {
            break;
        }

        let result = match events.recv_timeout(Duration::from_millis(20)) {
            Ok(SessionEvent::FastData(frame)) => ingest_fast(&shared, &frame),
            Ok(SessionEvent::NormalData(frame)) => ingest_normal(&shared, &frame),
            Ok(_) => continue,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        if let Err(error) = result {
            if let Ok(mut state) = shared.lock() {
                state.runtime_error = Some(error.to_string());
            }
            let mask = shared.lock().ok().map(|state| group_mask(&state)).unwrap_or(0);
            if mask != 0 {
                let _ = session.plot_stop(mask);
            }
            break;
        }
    }
}

fn promote_if_pending(group: &mut GroupRuntime, config_id: u8) -> Result<(), MixedScopeError> {
    if group.active.config_id == config_id {
        return Ok(());
    }

    if group.pending.as_ref().is_some_and(|pending| pending.config_id == config_id) {
        group.active = group.pending.take().expect("pending config checked");
        group.sequence.reset();
        return Ok(());
    }

    Err(MixedScopeError::UnknownConfig(config_id))
}

fn ingest_fast(
    shared: &Arc<Mutex<SharedState>>,
    frame: &nmixx_core::wire::CanFdFrame,
) -> Result<(), MixedScopeError> {
    let config_id = frame.data().get(2).copied().ok_or(MixedScopeError::UnknownConfig(0))?;
    let mut state = shared.lock().map_err(|_| MixedScopeError::Closed)?;
    let (ids, scales) = {
        let group = state.fast.as_mut().ok_or(MixedScopeError::UnknownConfig(config_id))?;
        promote_if_pending(group, config_id)?;
        let ids = group.active.ids.clone();
        let scales = group.active.scales.clone();

        let decoded = decode_fast_data(frame, ids.len())?;
        group.sequence.observe(decoded.sequence);
        group.lost_frames = group.sequence.lost_total();
        (ids, (scales, decoded))
    };

    let (scales, decoded) = scales;
    let values = decoded
        .dequantize(&scales)
        .ok_or(MixedScopeError::Runtime("FAST scale count mismatch".to_owned()))?;

    let channel_count = ids.len();
    for sample in values.chunks_exact(channel_count) {
        for (index, id) in ids.iter().enumerate() {
            if let Some(history) = state.histories.get_mut(id) {
                history.stream.push_sample(&[sample[index]])?;
            }
        }
    }
    Ok(())
}

fn ingest_normal(
    shared: &Arc<Mutex<SharedState>>,
    frame: &nmixx_core::wire::CanFdFrame,
) -> Result<(), MixedScopeError> {
    let decoded = decode_normal_data(frame)?;
    let mut state = shared.lock().map_err(|_| MixedScopeError::Closed)?;
    let ids = {
        let group = state
            .normal
            .as_mut()
            .ok_or(MixedScopeError::UnknownConfig(decoded.config_id))?;
        promote_if_pending(group, decoded.config_id)?;

        if decoded.values.len() != group.active.ids.len() {
            return Err(MixedScopeError::Runtime("NORMAL channel count mismatch".to_owned()));
        }

        group.sequence.observe(decoded.sequence);
        group.lost_frames = group.sequence.lost_total();
        group.active.ids.clone()
    };

    for (index, id) in ids.iter().enumerate() {
        if let Some(history) = state.histories.get_mut(id) {
            history.stream.push_sample(&[decoded.values[index]])?;
        }
    }
    Ok(())
}
