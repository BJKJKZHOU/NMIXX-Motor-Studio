use std::collections::VecDeque;
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

        Ok(samples.ceil() as usize)
    }

    /// Approximate in-memory payload size used by the current f32 sample store.
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
    pub samples: Vec<Vec<f32>>,
}

impl StreamSnapshot {
    pub fn duration(&self) -> Duration {
        if self.config.sample_rate_hz == 0 {
            return Duration::ZERO;
        }
        Duration::from_secs_f64(self.samples.len() as f64 / f64::from(self.config.sample_rate_hz))
    }
}

/// RAM-only stream history/capture runtime.
///
/// `Live` keeps a rolling history and overwrites the oldest samples when full.
/// `capture()` clears history and records a finite interval without overwriting
/// its beginning; once the target is reached the session becomes `Paused`.
/// Nothing in this type writes to disk. Exporters consume `snapshot()` later.
pub struct StreamSession {
    config: StreamConfig,
    capacity_samples: usize,
    samples: VecDeque<Vec<f32>>,
    state: StreamState,
    capture_target: Option<usize>,
}

impl StreamSession {
    pub fn new(config: StreamConfig) -> Result<Self, StreamError> {
        let capacity_samples = config.capacity_samples()?;
        Ok(Self {
            config,
            capacity_samples,
            samples: VecDeque::with_capacity(capacity_samples),
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
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
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
        self.samples.clear();
    }

    pub fn capture(&mut self, duration: Duration) -> Result<(), StreamError> {
        if duration.is_zero() {
            return Err(StreamError::InvalidCaptureDuration);
        }
        let requested = (duration.as_secs_f64() * f64::from(self.config.sample_rate_hz)).ceil() as usize;
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
            StreamState::Live => {
                if self.samples.len() == self.capacity_samples {
                    self.samples.pop_front();
                }
                self.samples.push_back(sample.to_vec());
            }
            StreamState::Capturing => {
                let target = self.capture_target.expect("capturing state must have target");
                if self.samples.len() >= target {
                    self.state = StreamState::Paused;
                    self.capture_target = None;
                    return Ok(false);
                }
                self.samples.push_back(sample.to_vec());
                if self.samples.len() == target {
                    self.state = StreamState::Paused;
                    self.capture_target = None;
                }
            }
        }

        Ok(true)
    }

    pub fn snapshot(&self) -> StreamSnapshot {
        StreamSnapshot {
            config: self.config.clone(),
            samples: self.samples.iter().cloned().collect(),
        }
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
        assert_eq!(snapshot.samples.len(), 4);
        assert_eq!(snapshot.samples[0], vec![2.0, -2.0]);
        assert_eq!(snapshot.samples[3], vec![5.0, -5.0]);
    }

    #[test]
    fn pause_freezes_current_buffer() {
        let mut stream = StreamSession::new(config()).unwrap();
        stream.live();
        stream.push_sample(&[1.0, 2.0]).unwrap();
        stream.pause();
        assert!(!stream.push_sample(&[3.0, 4.0]).unwrap());
        assert_eq!(stream.snapshot().samples, vec![vec![1.0, 2.0]]);
    }

    #[test]
    fn clear_while_live_restarts_history_from_now() {
        let mut stream = StreamSession::new(config()).unwrap();
        stream.live();
        stream.push_sample(&[1.0, 2.0]).unwrap();
        stream.clear();
        stream.push_sample(&[3.0, 4.0]).unwrap();
        assert_eq!(stream.snapshot().samples, vec![vec![3.0, 4.0]]);
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

        assert_eq!(stream.state(), StreamState::Paused);
        assert_eq!(stream.snapshot().samples, vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
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
