use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    Stopped,
    Live,
    Capturing,
    Paused,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamConfig {
    pub sample_rate_hz: u32,
    pub channel_count: usize,
    pub history: Duration,
}

impl StreamConfig {
    pub fn capacity_samples(&self) -> Result<usize, StreamError> {
        if self.sample_rate_hz == 0 {
            return Err(StreamError::InvalidConfig("sample_rate_hz must be greater than zero"));
        }
        if self.channel_count == 0 {
            return Err(StreamError::InvalidConfig("channel_count must be greater than zero"));
        }
        if self.history.is_zero() {
            return Err(StreamError::InvalidConfig("history must be greater than zero"));
        }

        let samples = self.history.as_secs_f64() * f64::from(self.sample_rate_hz);
        if !samples.is_finite() || samples > usize::MAX as f64 {
            return Err(StreamError::InvalidConfig("history is too large"));
        }

        let samples = samples.ceil() as usize;
        if samples == 0 {
            return Err(StreamError::InvalidConfig("history is shorter than one sample"));
        }
        Ok(samples)
    }

    pub fn estimated_ram_bytes(&self) -> Result<usize, StreamError> {
        self.capacity_samples()?
            .checked_mul(self.channel_count)
            .and_then(|values| values.checked_mul(std::mem::size_of::<f32>()))
            .ok_or(StreamError::InvalidConfig("stream buffer size overflows usize"))
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StreamError {
    #[error("invalid stream config: {0}")]
    InvalidConfig(&'static str),
    #[error("sample has {actual} channels, expected {expected}")]
    ChannelCount { expected: usize, actual: usize },
    #[error("capture duration must be greater than zero")]
    InvalidCaptureDuration,
    #[error("capture requires {requested} samples but buffer capacity is {capacity}")]
    CaptureTooLong { requested: usize, capacity: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamSnapshot {
    pub config: StreamConfig,
    /// Sample-major flat engineering values: sample0[ch0..], sample1[ch0..], ...
    pub values: Vec<f32>,
}

impl StreamSnapshot {
    pub fn sample_count(&self) -> usize {
        self.values.len() / self.config.channel_count
    }

    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.sample_count() as f64 / f64::from(self.config.sample_rate_hz))
    }

    pub fn sample(&self, index: usize) -> Option<&[f32]> {
        if index >= self.sample_count() {
            return None;
        }
        let start = index * self.config.channel_count;
        Some(&self.values[start..start + self.config.channel_count])
    }
}

/// RAM-only stream history/capture runtime.
///
/// Storage is one fixed flat allocation. Steady-state acquisition performs no
/// per-sample heap allocation. `Live` overwrites the oldest sample when full;
/// `capture()` clears history, records a finite interval, then pauses without
/// overwriting the capture start. Disk I/O is intentionally outside this type.
pub struct StreamSession {
    config: StreamConfig,
    capacity_samples: usize,
    storage: Vec<f32>,
    oldest_sample: usize,
    sample_len: usize,
    state: StreamState,
    capture_target: Option<usize>,
}

impl StreamSession {
    pub fn new(config: StreamConfig) -> Result<Self, StreamError> {
        let capacity_samples = config.capacity_samples()?;
        let values = capacity_samples
            .checked_mul(config.channel_count)
            .ok_or(StreamError::InvalidConfig("stream buffer size overflows usize"))?;
        Ok(Self {
            config,
            capacity_samples,
            storage: vec![0.0; values],
            oldest_sample: 0,
            sample_len: 0,
            state: StreamState::Stopped,
            capture_target: None,
        })
    }

    pub fn config(&self) -> &StreamConfig {
        &self.config
    }

    pub fn state(&self) -> StreamState {
        self.state
    }

    pub fn len(&self) -> usize {
        self.sample_len
    }

    pub fn is_empty(&self) -> bool {
        self.sample_len == 0
    }

    pub fn capacity_samples(&self) -> usize {
        self.capacity_samples
    }

    pub fn live(&mut self) {
        self.capture_target = None;
        self.state = StreamState::Live;
    }

    pub fn pause(&mut self) {
        self.capture_target = None;
        self.state = StreamState::Paused;
    }

    pub fn stop(&mut self) {
        self.capture_target = None;
        self.state = StreamState::Stopped;
    }

    pub fn resume(&mut self) {
        self.capture_target = None;
        self.state = StreamState::Live;
    }

    pub fn clear(&mut self) {
        self.oldest_sample = 0;
        self.sample_len = 0;
    }

    pub fn capture(&mut self, duration: Duration) -> Result<(), StreamError> {
        if duration.is_zero() {
            return Err(StreamError::InvalidCaptureDuration);
        }

        let requested_f = duration.as_secs_f64() * f64::from(self.config.sample_rate_hz);
        if !requested_f.is_finite() || requested_f > usize::MAX as f64 {
            return Err(StreamError::CaptureTooLong {
                requested: usize::MAX,
                capacity: self.capacity_samples,
            });
        }
        let requested = requested_f.ceil() as usize;
        if requested == 0 {
            return Err(StreamError::InvalidCaptureDuration);
        }
        if requested > self.capacity_samples {
            return Err(StreamError::CaptureTooLong {
                requested,
                capacity: self.capacity_samples,
            });
        }

        self.clear();
        self.capture_target = Some(requested);
        self.state = StreamState::Capturing;
        Ok(())
    }

    /// Adds one engineering-value sample. Returns true when the sample was kept.
    /// Paused/stopped sessions deliberately ignore incoming values.
    pub fn push_sample(&mut self, sample: &[f32]) -> Result<bool, StreamError> {
        if sample.len() != self.config.channel_count {
            return Err(StreamError::ChannelCount {
                expected: self.config.channel_count,
                actual: sample.len(),
            });
        }

        match self.state {
            StreamState::Stopped | StreamState::Paused => return Ok(false),
            StreamState::Live => self.push_live(sample),
            StreamState::Capturing => {
                let target = self.capture_target.expect("capturing state must have target");
                if self.sample_len >= target {
                    self.state = StreamState::Paused;
                    self.capture_target = None;
                    return Ok(false);
                }
                self.push_without_overwrite(sample);
                if self.sample_len == target {
                    self.state = StreamState::Paused;
                    self.capture_target = None;
                }
            }
        }

        Ok(true)
    }

    pub fn snapshot(&self) -> StreamSnapshot {
        self.snapshot_tail(self.sample_len)
    }

    /// Copies only the newest `max_samples` in logical time order.
    /// This is intended for UI refreshes so the full rolling history does not
    /// need to be cloned on every frame.
    pub fn snapshot_tail(&self, max_samples: usize) -> StreamSnapshot {
        self.snapshot_window(max_samples, 0)
    }

    /// Copies a logical window without first cloning the full history.
    /// `end_offset_samples == 0` addresses the newest sample; larger offsets
    /// move the window toward the beginning of the retained recording.
    pub fn snapshot_window(&self, max_samples: usize, end_offset_samples: usize) -> StreamSnapshot {
        let available = self.sample_len;
        let end = available.saturating_sub(end_offset_samples.min(available));
        let start = end.saturating_sub(max_samples.min(end));
        let keep = end.saturating_sub(start);
        let mut values = Vec::with_capacity(keep * self.config.channel_count);
        for logical in start..end {
            let physical = (self.oldest_sample + logical) % self.capacity_samples;
            let physical_start = physical * self.config.channel_count;
            values.extend_from_slice(
                &self.storage[physical_start..physical_start + self.config.channel_count],
            );
        }
        StreamSnapshot {
            config: self.config.clone(),
            values,
        }
    }

    fn push_live(&mut self, sample: &[f32]) {
        if self.sample_len < self.capacity_samples {
            self.push_without_overwrite(sample);
            return;
        }

        self.write_physical(self.oldest_sample, sample);
        self.oldest_sample = (self.oldest_sample + 1) % self.capacity_samples;
    }

    fn push_without_overwrite(&mut self, sample: &[f32]) {
        debug_assert!(self.sample_len < self.capacity_samples);
        let physical = (self.oldest_sample + self.sample_len) % self.capacity_samples;
        self.write_physical(physical, sample);
        self.sample_len += 1;
    }

    fn write_physical(&mut self, sample_index: usize, sample: &[f32]) {
        let start = sample_index * self.config.channel_count;
        self.storage[start..start + self.config.channel_count].copy_from_slice(sample);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> StreamConfig {
        StreamConfig {
            sample_rate_hz: 4,
            channel_count: 2,
            history: Duration::from_secs(1),
        }
    }

    #[test]
    fn live_mode_overwrites_oldest_samples() {
        let mut stream = StreamSession::new(config()).unwrap();
        stream.live();
        for n in 0..6 {
            stream.push_sample(&[n as f32, -(n as f32)]).unwrap();
        }

        let snapshot = stream.snapshot();
        assert_eq!(snapshot.sample_count(), 4);
        assert_eq!(snapshot.sample(0).unwrap(), &[2.0, -2.0]);
        assert_eq!(snapshot.sample(3).unwrap(), &[5.0, -5.0]);
    }

    #[test]
    fn tail_snapshot_keeps_only_newest_samples_in_order() {
        let mut stream = StreamSession::new(config()).unwrap();
        stream.live();
        for n in 0..6 {
            stream.push_sample(&[n as f32, -(n as f32)]).unwrap();
        }
        let snapshot = stream.snapshot_tail(2);
        assert_eq!(snapshot.sample_count(), 2);
        assert_eq!(snapshot.sample(0).unwrap(), &[4.0, -4.0]);
        assert_eq!(snapshot.sample(1).unwrap(), &[5.0, -5.0]);
    }

    #[test]
    fn window_snapshot_reads_offset_range_without_full_copy_semantics() {
        let mut stream = StreamSession::new(config()).unwrap();
        stream.live();
        for n in 0..6 {
            stream.push_sample(&[n as f32, -(n as f32)]).unwrap();
        }

        let snapshot = stream.snapshot_window(2, 1);
        assert_eq!(snapshot.sample_count(), 2);
        assert_eq!(snapshot.sample(0).unwrap(), &[3.0, -3.0]);
        assert_eq!(snapshot.sample(1).unwrap(), &[4.0, -4.0]);
    }

    #[test]
    fn pause_freezes_current_buffer() {
        let mut stream = StreamSession::new(config()).unwrap();
        stream.live();
        stream.push_sample(&[1.0, 2.0]).unwrap();
        stream.pause();
        assert!(!stream.push_sample(&[3.0, 4.0]).unwrap());
        assert_eq!(stream.snapshot().sample(0).unwrap(), &[1.0, 2.0]);
    }

    #[test]
    fn clear_while_live_restarts_history_from_now() {
        let mut stream = StreamSession::new(config()).unwrap();
        stream.live();
        stream.push_sample(&[1.0, 2.0]).unwrap();
        stream.clear();
        stream.push_sample(&[3.0, 4.0]).unwrap();
        let snapshot = stream.snapshot();
        assert_eq!(snapshot.sample_count(), 1);
        assert_eq!(snapshot.sample(0).unwrap(), &[3.0, 4.0]);
        assert_eq!(stream.state(), StreamState::Live);
    }

    #[test]
    fn finite_capture_clears_then_pauses_at_target() {
        let mut stream = StreamSession::new(config()).unwrap();
        stream.live();
        stream.push_sample(&[9.0, 9.0]).unwrap();

        stream.capture(Duration::from_millis(500)).unwrap();
        assert_eq!(stream.state(), StreamState::Capturing);
        stream.push_sample(&[1.0, 2.0]).unwrap();
        stream.push_sample(&[3.0, 4.0]).unwrap();

        let snapshot = stream.snapshot();
        assert_eq!(stream.state(), StreamState::Paused);
        assert_eq!(snapshot.sample_count(), 2);
        assert_eq!(snapshot.sample(0).unwrap(), &[1.0, 2.0]);
        assert_eq!(snapshot.sample(1).unwrap(), &[3.0, 4.0]);
    }

    #[test]
    fn capture_cannot_exceed_ring_capacity() {
        let mut stream = StreamSession::new(config()).unwrap();
        assert_eq!(
            stream.capture(Duration::from_secs(2)),
            Err(StreamError::CaptureTooLong {
                requested: 8,
                capacity: 4,
            })
        );
    }

    #[test]
    fn snapshot_reports_duration_and_ram_estimate() {
        let mut stream = StreamSession::new(config()).unwrap();
        assert_eq!(stream.config().estimated_ram_bytes().unwrap(), 32);
        stream.live();
        stream.push_sample(&[1.0, 2.0]).unwrap();
        stream.push_sample(&[3.0, 4.0]).unwrap();
        assert_eq!(stream.snapshot().duration(), Duration::from_millis(500));
    }
}
