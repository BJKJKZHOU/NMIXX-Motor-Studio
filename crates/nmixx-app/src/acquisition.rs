//! One device stream, shared baseline telemetry, and independently frozen records.
//! View selection is not a device channel allocation. Only the merged demand is
//! sent to Plot; a faster existing source also serves slower consumers by decimation.
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::mixed_scope::{build_config, SampleSink};
use crate::{DevicePlotCapabilities, DeviceSession, HostSchema, MixedScopeChannel,
    MixedScopeConfig, MixedScopeError, MixedScopeSeries, MixedScopeSession,
    MixedScopeSnapshot, MixedScopeStatus, ParameterService, ScopeRate,
    ScopeSelection, StreamConfig, StreamSession, StreamState};

pub(crate) const BASE_SYMBOLS: &[&str] = &[
    "PARAM_RUN_IQ", "PARAM_RUN_WM", "PARAM_RUN_POSITION", "PARAM_ADC_VBUS",
];
const RECENT_HISTORY: Duration = Duration::from_secs(10);
const RECORD_BUDGET: usize = 128 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum View { Scope, Tuning }

struct Channel {
    meta: MixedScopeChannel,
    stream: StreamSession,
    phase: u32,
    source_hz: u32,
}
impl Channel {
    fn new(meta: MixedScopeChannel, history: Duration) -> Result<Self, MixedScopeError> {
        let stream = StreamSession::new(StreamConfig {
            sample_rate_hz: meta.sample_rate_hz, channel_count: 1, history,
        })?;
        Ok(Self { meta, stream, phase: 0, source_hz: 0 })
    }
    fn push(&mut self, value: f32, source_hz: u32) -> Result<(), MixedScopeError> {
        let target = self.meta.sample_rate_hz;
        if source_hz < target || source_hz % target != 0 {
            return Err(MixedScopeError::Runtime("incompatible shared sample rates".to_owned()));
        }
        if self.source_hz != source_hz { self.source_hz = source_hz; self.phase = 0; }
        self.phase += target;
        if self.phase >= source_hz {
            self.phase -= source_hz;
            self.stream.push_sample(&[value])?;
        }
        Ok(())
    }
}

struct Record {
    config: MixedScopeConfig,
    channels: HashMap<u16, Channel>,
    state: StreamState,
    frozen_losses: Option<u64>,
}
impl Record {
    fn new(config: MixedScopeConfig, stored: &[MixedScopeChannel]) -> Result<Self, MixedScopeError> {
        check_budget(stored, config.history)?;
        let mut channels = HashMap::new();
        for meta in stored {
            channels.insert(meta.id, Channel::new(meta.clone(), config.history)?);
        }
        Ok(Self { config, channels, state: StreamState::Stopped, frozen_losses: None })
    }
    fn active(&self) -> bool { matches!(self.state(), StreamState::Live | StreamState::Capturing) }
    fn state(&self) -> StreamState {
        if self.state == StreamState::Capturing && !self.channels.is_empty()
            && self.channels.values().all(|channel| channel.stream.state() == StreamState::Paused) {
            StreamState::Paused
        } else { self.state }
    }
    fn live(&mut self) {
        self.state = StreamState::Live;
        self.frozen_losses = None;
        for channel in self.channels.values_mut() { channel.stream.live(); }
    }
    fn stop(&mut self, state: StreamState) {
        self.state = state;
        for channel in self.channels.values_mut() { channel.stream.stop(); }
    }
    fn clear(&mut self) { for channel in self.channels.values_mut() { channel.stream.clear(); } }
    fn capture(&mut self, duration: Duration) -> Result<(), MixedScopeError> {
        for channel in self.channels.values_mut() { channel.stream.capture(duration)?; }
        self.state = StreamState::Capturing;
        self.frozen_losses = None;
        Ok(())
    }
    fn feed(&mut self, id: u16, value: f32, source_hz: u32) -> Result<(), MixedScopeError> {
        if self.active() {
            if let Some(channel) = self.channels.get_mut(&id) { channel.push(value, source_hz)?; }
        }
        Ok(())
    }
    fn seed_baseline(&mut self, baseline: &Record) -> Result<(), MixedScopeError> {
        for (id, channel) in &mut self.channels {
            let Some(source) = baseline.channels.get(id) else { continue; };
            if source.meta.sample_rate_hz != channel.meta.sample_rate_hz { continue; }
            let snapshot = source.stream.snapshot_tail(channel.stream.capacity_samples());
            channel.stream.clear();
            channel.stream.live();
            for value in snapshot.values { channel.stream.push_sample(&[value])?; }
            if !matches!(self.state, StreamState::Live | StreamState::Capturing) { channel.stream.stop(); }
        }
        Ok(())
    }
    fn status(&self, lost_frames: u64) -> MixedScopeStatus {
        let mut samples = 0;
        let mut capacity_samples = 0;
        for meta in &self.config.channels {
            if let Some(channel) = self.channels.get(&meta.id) {
                samples += channel.stream.len();
                capacity_samples += channel.stream.capacity_samples();
            }
        }
        MixedScopeStatus { state: self.state(), samples, capacity_samples, lost_frames: self.frozen_losses.unwrap_or(lost_frames) }
    }
    fn duration(&self) -> Duration {
        let seconds = self.config.channels.iter().filter_map(|meta| self.channels.get(&meta.id))
            .map(|channel| channel.stream.len() as f64 / f64::from(channel.meta.sample_rate_hz))
            .fold(0.0_f64, f64::max);
        Duration::from_secs_f64(seconds)
    }
    fn snapshot(&self, window: Duration, offset: Duration, lost_frames: u64) -> MixedScopeSnapshot {
        let series = self.config.channels.iter().map(|meta| {
            let values = self.channels.get(&meta.id).filter(|channel| channel.meta.rate == meta.rate)
                .map(|channel| channel.stream.snapshot_window(
                    (window.as_secs_f64() * f64::from(meta.sample_rate_hz)).ceil().max(1.0) as usize,
                    (offset.as_secs_f64() * f64::from(meta.sample_rate_hz)).round() as usize,
                ).values).unwrap_or_default();
            MixedScopeSeries { id: meta.id, sample_rate_hz: meta.sample_rate_hz, values }
        }).collect();
        MixedScopeSnapshot { state: self.state(), lost_frames: self.frozen_losses.unwrap_or(lost_frames), series }
    }
}

fn check_budget(channels: &[MixedScopeChannel], history: Duration) -> Result<(), MixedScopeError> {
    let bytes = channels.iter().try_fold(0usize, |sum, channel| {
        let config = StreamConfig { sample_rate_hz: channel.sample_rate_hz, channel_count: 1, history };
        let bytes = config.estimated_ram_bytes()?;
        sum.checked_add(bytes).ok_or(crate::StreamError::InvalidConfig("record buffer size overflow"))
    })?;
    if bytes > RECORD_BUDGET { return Err(MixedScopeError::Runtime("record exceeds the 128 MiB limit".to_owned())); }
    Ok(())
}

fn selections(config: &MixedScopeConfig) -> Vec<ScopeSelection> {
    config.channels.iter().map(|channel| ScopeSelection { id: channel.id, rate: channel.rate }).collect()
}

/// Sorted and de-duplicated by parameter ID. FAST supersedes NORMAL; the receiving
/// side produces NORMAL samples locally, so the same ID is never sent twice.
fn merge_demands(groups: &[&[ScopeSelection]]) -> Vec<ScopeSelection> {
    let mut merged = BTreeMap::new();
    for group in groups {
        for selection in *group {
            merged.entry(selection.id).and_modify(|rate| {
                if selection.rate == ScopeRate::Fast { *rate = ScopeRate::Fast; }
            }).or_insert(selection.rate);
        }
    }
    merged.into_iter().map(|(id, rate)| ScopeSelection { id, rate }).collect()
}

struct CaptureData {
    baseline: Record,
    scope: Record,
    tuning: Option<Record>,
    actual: Vec<ScopeSelection>,
    error: Option<String>,
}
impl CaptureData {
    fn record(&self, view: View) -> Result<&Record, MixedScopeError> {
        match view {
            View::Scope => Ok(&self.scope),
            View::Tuning => self.tuning.as_ref().ok_or_else(|| MixedScopeError::Runtime("no tuning recording".to_owned())),
        }
    }
    fn record_mut(&mut self, view: View) -> Result<&mut Record, MixedScopeError> {
        match view {
            View::Scope => Ok(&mut self.scope),
            View::Tuning => self.tuning.as_mut().ok_or_else(|| MixedScopeError::Runtime("no tuning recording".to_owned())),
        }
    }
    fn demand(&self, view: View, config: &MixedScopeConfig, active: bool) -> Vec<ScopeSelection> {
        let base = selections(&self.baseline.config);
        let requested = if active { selections(config) } else { Vec::new() };
        let other = match view {
            View::Scope => self.tuning.as_ref(),
            View::Tuning => Some(&self.scope),
        };
        let other = other.filter(|record| record.active()).map(|record| selections(&record.config)).unwrap_or_default();
        merge_demands(&[&base, &requested, &other])
    }
    fn feed(&mut self, rate: ScopeRate, ids: &[u16], values: &[f32], hz: u32) -> Result<(), MixedScopeError> {
        for sample in values.chunks_exact(ids.len()) {
            for (&id, &value) in ids.iter().zip(sample) {
                // Old-format frames may be in flight during a rate change. They
                // are decoded by their own layout but cannot duplicate samples.
                if !self.actual.iter().any(|item| item.id == id && item.rate == rate) { continue; }
                self.baseline.feed(id, value, hz)?;
                self.scope.feed(id, value, hz)?;
                if let Some(tuning) = &mut self.tuning { tuning.feed(id, value, hz)?; }
            }
        }
        Ok(())
    }
}

pub(crate) struct SharedAcquisition {
    source: MixedScopeSession,
    data: Arc<Mutex<CaptureData>>,
    capabilities: DevicePlotCapabilities,
    schema: HostSchema,
    parameters: ParameterService,
}
impl SharedAcquisition {
    pub(crate) fn new(session: DeviceSession, capabilities: DevicePlotCapabilities,
        schema: HostSchema, parameters: ParameterService) -> Result<Self, MixedScopeError> {
        let base = BASE_SYMBOLS.iter().filter_map(|symbol| {
            let meta = schema.parameter_by_key(symbol)?;
            capabilities.channel(meta.id).filter(|channel| channel.supports_normal())
                .map(|_| ScopeSelection { id: meta.id, rate: ScopeRate::Normal })
        }).collect::<Vec<_>>();
        let base = merge_demands(&[&base]);
        let config = build_config(&capabilities, &schema, &base, RECENT_HISTORY, 1)?;
        let mut baseline = Record::new(config.clone(), &config.channels)?;
        baseline.live();
        let mut scope = Record::new(config.clone(), &config.channels)?;
        scope.live();
        let data = Arc::new(Mutex::new(CaptureData {
            baseline, scope, tuning: None, actual: base.clone(), error: None,
        }));
        // The common records own history. The transport decoder needs only one
        // block of scratch history, not another 128 MiB acquisition buffer.
        let source = MixedScopeSession::from_capabilities(session, &capabilities, &schema,
            &base, Duration::from_millis(1), 1)?;
        let sink_data = data.clone();
        let failed_data = data.clone();
        let failed_parameters = parameters.clone();
        let base_ids = base.iter().map(|item| item.id).collect::<Vec<_>>();
        let source_caps = capabilities.clone();
        let stream_parameters = parameters.clone();
        source.set_sample_sink(SampleSink {
            samples: Arc::new(move |rate, ids, values| {
                if ids.is_empty() { return Ok(()); }
                let hz = match rate { ScopeRate::Fast => source_caps.fast_rate_hz, ScopeRate::Normal => source_caps.normal_rate_hz };
                let mut store = sink_data.lock().map_err(|_| MixedScopeError::Closed)?;
                if !store.baseline.active() { return Ok(()); }
                store.feed(rate, ids, values, hz)?;
                if let Some(last) = values.chunks_exact(ids.len()).last() {
                    let latest = ids.iter().zip(last).filter(|(id, _)| base_ids.contains(id)
                        && store.actual.iter().any(|item| item.id == **id && item.rate == rate))
                        .map(|(id, value)| (*id, *value)).collect::<Vec<_>>();
                    // Position Plot values are f32 total turns, not a typed
                    // turn+rad readback. ParameterService explicitly rejects them.
                    stream_parameters.ingest_stream_values(&latest).map_err(|error| MixedScopeError::Runtime(error.to_string()))?;
                }
                Ok(())
            }),
            failed: Arc::new(move |message| {
                if let Ok(mut store) = failed_data.lock() { store.error = Some(message.to_owned()); }
                failed_parameters.invalidate_stream_values(message);
            }),
        })?;
        source.live()?;
        Ok(Self { source, data, capabilities, schema, parameters })
    }
    pub(crate) fn base_ids(&self) -> Result<Vec<u16>, MixedScopeError> {
        Ok(self.data.lock().map_err(|_| MixedScopeError::Closed)?.baseline.config.channels.iter().map(|channel| channel.id).collect())
    }
    fn healthy(&self) -> Result<(), MixedScopeError> {
        if let Some(message) = &self.data.lock().map_err(|_| MixedScopeError::Closed)?.error {
            return Err(MixedScopeError::Runtime(message.clone()));
        }
        Ok(())
    }
    fn apply_demand(&self, demand: &[ScopeSelection]) -> Result<(), MixedScopeError> {
        // Validate real transport capacity, including hidden baseline channels.
        build_config(&self.capabilities, &self.schema, demand, Duration::from_millis(1), 1)?;
        if self.capabilities.fast_rate_hz == 0 || self.capabilities.normal_rate_hz == 0
            || self.capabilities.fast_rate_hz % self.capabilities.normal_rate_hz != 0 {
            return Err(MixedScopeError::Runtime("FAST rate must be an integer multiple of NORMAL".to_owned()));
        }
        let same = self.data.lock().map_err(|_| MixedScopeError::Closed)?.actual == demand;
        if same { return Ok(()); }
        self.source.reconfigure(demand)?;
        // The caller commits actual demand together with the consumer layout.
        // A decoder callback between those updates must not feed a new rate
        // into the old recording layout.
        Ok(())
    }
    pub(crate) fn configure(&self, view: View, selected: &[ScopeSelection], history: Duration) -> Result<MixedScopeConfig, MixedScopeError> {
        self.healthy()?;
        let unique = merge_demands(&[selected]);
        // An empty Scope view simply hides every curve. Tuning still requires a channel.
        let config = if unique.is_empty() && view == View::Scope {
            MixedScopeConfig { config_id: 1, history, channels: Vec::new() }
        } else { build_config(&self.capabilities, &self.schema, &unique, history, 1)? };
        let (active, demand, stored) = {
            let data = self.data.lock().map_err(|_| MixedScopeError::Closed)?;
            let active = view == View::Scope && data.scope.active();
            let demand = data.demand(view, &config, active);
            let stored = if view == View::Scope {
                let mut channels = data.baseline.config.channels.iter().map(|channel| (channel.id, channel.clone())).collect::<BTreeMap<_, _>>();
                for channel in &config.channels { channels.insert(channel.id, channel.clone()); }
                channels.into_values().collect()
            } else { config.channels.clone() };
            (active, demand, stored)
        };
        check_budget(&stored, history)?;
        self.apply_demand(&demand)?;
        let mut data = self.data.lock().map_err(|_| MixedScopeError::Closed)?;
        if view == View::Tuning {
            data.tuning = Some(Record::new(config.clone(), &stored)?);
        } else if !active {
            // A stopped record is immutable. Selection can reveal stored channels
            // but must not replace its samples or fetch live baseline history.
            data.scope.config = config.clone();
        } else {
            let mut previous = std::mem::take(&mut data.scope.channels);
            let mut channels = HashMap::new();
            for meta in stored {
                let reusable = previous.remove(&meta.id).filter(|old| old.meta.rate == meta.rate
                    && old.stream.config().history == history);
                let mut channel = if let Some(old) = reusable { old } else {
                    let mut channel = Channel::new(meta.clone(), history)?;
                    if active {
                        channel.stream.live();
                        if let Some(base) = data.baseline.channels.get(&meta.id).filter(|base| base.meta.rate == meta.rate) {
                            for value in base.stream.snapshot_tail(channel.stream.capacity_samples()).values {
                                channel.stream.push_sample(&[value])?;
                            }
                        }
                    }
                    channel
                };
                channel.meta = meta;
                channels.insert(channel.meta.id, channel);
            }
            data.scope.channels = channels;
            data.scope.config = config.clone();
        }
        data.actual = demand;
        Ok(config)
    }
    pub(crate) fn live(&self, view: View) -> Result<(), MixedScopeError> {
        self.healthy()?;
        let demand = {
            let data = self.data.lock().map_err(|_| MixedScopeError::Closed)?;
            data.demand(view, &data.record(view)?.config, true)
        };
        self.apply_demand(&demand)?;
        let mut data = self.data.lock().map_err(|_| MixedScopeError::Closed)?;
        if data.record(view)?.active() { data.actual = demand; return Ok(()); }
        if view == View::Scope {
            // Selection may change while a frozen record is inspected. Allocate
            // the new layout only on Run, not while looking at that recording.
            let config = data.scope.config.clone();
            let mut stored = data.baseline.config.channels.iter()
                .map(|channel| (channel.id, channel.clone())).collect::<BTreeMap<_, _>>();
            for channel in &config.channels { stored.insert(channel.id, channel.clone()); }
            let mut record = Record::new(config, &stored.into_values().collect::<Vec<_>>())?;
            record.live();
            record.seed_baseline(&data.baseline)?;
            data.scope = record;
        } else {
            let record = data.record_mut(view)?;
            record.clear();
            record.live();
        }
        data.actual = demand;
        Ok(())
    }
    pub(crate) fn stop(&self, view: View, state: StreamState) -> Result<(), MixedScopeError> {
        let lost = self.source.status().map(|status| status.lost_frames).unwrap_or(0);
        let demand = {
            let mut data = self.data.lock().map_err(|_| MixedScopeError::Closed)?;
            let record = data.record_mut(view)?;
            if record.frozen_losses.is_none() { record.frozen_losses = Some(lost); }
            record.stop(state);
            data.demand(view, &data.record(view)?.config, false)
        };
        // Frozen records no longer receive samples. The baseline remains live;
        // this path never sends a motor action or stops the whole NORMAL group.
        self.apply_demand(&demand)?;
        self.data.lock().map_err(|_| MixedScopeError::Closed)?.actual = demand;
        Ok(())
    }
    pub(crate) fn clear(&self, view: View) -> Result<(), MixedScopeError> {
        self.data.lock().map_err(|_| MixedScopeError::Closed)?.record_mut(view)?.clear();
        Ok(())
    }
    pub(crate) fn capture(&self, view: View, duration: Duration) -> Result<(), MixedScopeError> {
        self.healthy()?;
        let demand = {
            let data = self.data.lock().map_err(|_| MixedScopeError::Closed)?;
            let record = data.record(view)?;
            if duration.is_zero() || duration > record.config.history {
                return Err(MixedScopeError::Runtime("capture duration exceeds recording capacity".to_owned()));
            }
            data.demand(view, &record.config, true)
        };
        self.apply_demand(&demand)?;
        let mut data = self.data.lock().map_err(|_| MixedScopeError::Closed)?;
        data.record_mut(view)?.capture(duration)?;
        data.actual = demand;
        Ok(())
    }
    pub(crate) fn config(&self, view: View) -> Result<MixedScopeConfig, MixedScopeError> {
        Ok(self.data.lock().map_err(|_| MixedScopeError::Closed)?.record(view)?.config.clone())
    }
    pub(crate) fn status(&self, view: View) -> Result<MixedScopeStatus, MixedScopeError> {
        let lost = self.source.status().map(|status| status.lost_frames).unwrap_or(0);
        let data = self.data.lock().map_err(|_| MixedScopeError::Closed)?;
        let record = data.record(view)?;
        if record.active() {
            if let Some(message) = &data.error { return Err(MixedScopeError::Runtime(message.clone())); }
        }
        Ok(record.status(lost))
    }
    pub(crate) fn recorded_duration(&self, view: View) -> Result<Duration, MixedScopeError> {
        Ok(self.data.lock().map_err(|_| MixedScopeError::Closed)?.record(view)?.duration())
    }
    pub(crate) fn snapshot(&self, view: View, window: Duration, offset: Duration) -> Result<MixedScopeSnapshot, MixedScopeError> {
        let lost = self.source.status().map(|status| status.lost_frames).unwrap_or(0);
        Ok(self.data.lock().map_err(|_| MixedScopeError::Closed)?.record(view)?.snapshot(window, offset, lost))
    }
    pub(crate) fn shutdown(&self) -> Result<(), MixedScopeError> {
        let mut data = self.data.lock().map_err(|_| MixedScopeError::Closed)?;
        data.baseline.stop(StreamState::Stopped);
        data.scope.stop(StreamState::Stopped);
        if let Some(tuning) = &mut data.tuning { tuning.stop(StreamState::Stopped); }
        drop(data);
        let result = self.source.stop();
        self.parameters.invalidate_stream_values("acquisition stopped");
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn selection(id: u16, rate: ScopeRate) -> ScopeSelection { ScopeSelection { id, rate } }
    fn config() -> MixedScopeConfig {
        MixedScopeConfig { config_id: 1, history: Duration::from_secs(1), channels: vec![MixedScopeChannel {
            id: 17, label: "Iq".into(), unit: Some("A".into()), rate: ScopeRate::Normal, sample_rate_hz: 4, scale: 1.0,
        }] }
    }
    #[test]
    fn visible_baseline_never_adds_a_device_channel() {
        let base = vec![selection(17, ScopeRate::Normal), selection(4, ScopeRate::Normal)];
        let expected = merge_demands(&[&base]);
        assert_eq!(merge_demands(&[&base, &[selection(17, ScopeRate::Normal)]]), expected);
        assert_eq!(merge_demands(&[&base, &[]]), expected);
    }
    #[test]
    fn fastest_consumer_wins_without_duplicate_ids() {
        assert_eq!(merge_demands(&[&[selection(17, ScopeRate::Normal)], &[selection(17, ScopeRate::Fast)]]),
            vec![selection(17, ScopeRate::Fast)]);
    }
    #[test]
    fn one_batch_updates_two_views_but_stop_freezes_only_one() {
        let config = config();
        let mut baseline = Record::new(config.clone(), &config.channels).unwrap(); baseline.live();
        let mut scope = Record::new(config.clone(), &config.channels).unwrap(); scope.live();
        let mut tuning = Record::new(config.clone(), &config.channels).unwrap(); tuning.capture(Duration::from_secs(1)).unwrap();
        let mut data = CaptureData { baseline, scope, tuning: Some(tuning), actual: selections(&config), error: None };
        data.feed(ScopeRate::Normal, &[17], &[1., 2.], 4).unwrap();
        data.scope.stop(StreamState::Stopped);
        for value in 3..10 { data.feed(ScopeRate::Normal, &[17], &[value as f32], 4).unwrap(); }
        assert_eq!(data.scope.snapshot(Duration::from_secs(1), Duration::ZERO, 0).series[0].values, vec![1., 2.]);
        assert_eq!(data.baseline.snapshot(Duration::from_secs(1), Duration::ZERO, 0).series[0].values, vec![6., 7., 8., 9.]);
        assert_eq!(data.tuning.as_ref().unwrap().snapshot(Duration::from_secs(1), Duration::ZERO, 0).series[0].values, vec![1., 2., 3., 4.]);
    }
    #[test]
    fn normal_consumer_decimates_a_fast_stream_without_interpolation() {
        let config = config();
        let mut record = Record::new(config.clone(), &config.channels).unwrap(); record.live();
        for value in 1..=8 { record.feed(17, value as f32, 8).unwrap(); }
        assert_eq!(record.snapshot(Duration::from_secs(1), Duration::ZERO, 0).series[0].values, vec![2., 4., 6., 8.]);
    }
    #[test]
    fn baseline_history_is_available_when_a_view_starts_later() {
        let config = config();
        let mut baseline = Record::new(config.clone(), &config.channels).unwrap(); baseline.live();
        for value in 1..=6 { baseline.feed(17, value as f32, 4).unwrap(); }
        let mut scope = Record::new(config.clone(), &config.channels).unwrap(); scope.live();
        scope.seed_baseline(&baseline).unwrap();
        assert_eq!(scope.snapshot(Duration::from_secs(1), Duration::ZERO, 0).series[0].values, vec![3., 4., 5., 6.]);
    }
}

#[cfg(test)]
#[path = "acquisition_tests.rs"]
mod transport_tests;
