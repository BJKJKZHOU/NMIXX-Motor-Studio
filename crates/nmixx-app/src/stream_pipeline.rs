use nmixx_core::protocol::{
    SequenceStatus, SequenceTracker, StreamDecodeError, decode_fast_data, decode_normal_data,
};
use nmixx_core::wire::CanFdFrame;
use thiserror::Error;

use crate::{StreamError, StreamSession, StreamSnapshot};

#[derive(Debug, Clone, PartialEq)]
pub enum StreamWireMode {
    Fast { scales: Vec<f32> },
    Normal,
}

#[derive(Debug, Error)]
pub enum StreamPipelineError {
    #[error(transparent)]
    Decode(#[from] StreamDecodeError),
    #[error(transparent)]
    Buffer(#[from] StreamError),
    #[error("stream Config_ID mismatch: expected {expected}, got {actual}")]
    ConfigId { expected: u8, actual: u8 },
    #[error("stream channel count mismatch: expected {expected}, got {actual}")]
    ChannelCount { expected: usize, actual: usize },
    #[error("FAST stream requires one scale per channel: expected {expected}, got {actual}")]
    ScaleCount { expected: usize, actual: usize },
    #[error("wrong frame type for configured stream mode")]
    WrongMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamIngestReport {
    pub sequence: SequenceStatus,
    pub samples_received: usize,
    pub samples_stored: usize,
    pub lost_frames_total: u64,
}

/// Application data-plane adapter from firmware telemetry frames into the
/// RAM-only `StreamSession` ring buffer.
///
/// This type performs no device I/O and no disk I/O. `DeviceSession` remains
/// the sole transport owner; callers feed its FAST/NORMAL events here.
pub struct StreamPipeline {
    config_id: u8,
    mode: StreamWireMode,
    sequence: SequenceTracker,
    stream: StreamSession,
}

impl StreamPipeline {
    pub fn new(
        config_id: u8,
        mode: StreamWireMode,
        stream: StreamSession,
    ) -> Result<Self, StreamPipelineError> {
        if let StreamWireMode::Fast { scales } = &mode {
            if scales.len() != stream.config().channel_count {
                return Err(StreamPipelineError::ScaleCount {
                    expected: stream.config().channel_count,
                    actual: scales.len(),
                });
            }
        }
        Ok(Self {
            config_id,
            mode,
            sequence: SequenceTracker::default(),
            stream,
        })
    }

    pub fn stream(&self) -> &StreamSession {
        &self.stream
    }

    pub fn stream_mut(&mut self) -> &mut StreamSession {
        &mut self.stream
    }

    pub fn snapshot(&self) -> StreamSnapshot {
        self.stream.snapshot()
    }

    pub fn reset_sequence(&mut self) {
        self.sequence.reset();
    }

    pub fn ingest_fast(
        &mut self,
        frame: &CanFdFrame,
    ) -> Result<StreamIngestReport, StreamPipelineError> {
        let StreamWireMode::Fast { scales } = &self.mode else {
            return Err(StreamPipelineError::WrongMode);
        };
        let channels = self.stream.config().channel_count;
        let decoded = decode_fast_data(frame, channels)?;
        if decoded.config_id != self.config_id {
            return Err(StreamPipelineError::ConfigId {
                expected: self.config_id,
                actual: decoded.config_id,
            });
        }

        let sequence = self.sequence.observe(decoded.sequence);
        let values = decoded
            .dequantize(scales)
            .ok_or(StreamPipelineError::ScaleCount {
                expected: channels,
                actual: scales.len(),
            })?;

        let mut stored = 0usize;
        for sample in values.chunks_exact(channels) {
            if self.stream.push_sample(sample)? {
                stored += 1;
            }
        }

        Ok(StreamIngestReport {
            sequence,
            samples_received: decoded.sample_count as usize,
            samples_stored: stored,
            lost_frames_total: self.sequence.lost_total(),
        })
    }

    pub fn ingest_normal(
        &mut self,
        frame: &CanFdFrame,
    ) -> Result<StreamIngestReport, StreamPipelineError> {
        if !matches!(self.mode, StreamWireMode::Normal) {
            return Err(StreamPipelineError::WrongMode);
        }
        let decoded = decode_normal_data(frame)?;
        if decoded.config_id != self.config_id {
            return Err(StreamPipelineError::ConfigId {
                expected: self.config_id,
                actual: decoded.config_id,
            });
        }
        let expected = self.stream.config().channel_count;
        if decoded.values.len() != expected {
            return Err(StreamPipelineError::ChannelCount {
                expected,
                actual: decoded.values.len(),
            });
        }

        let sequence = self.sequence.observe(decoded.sequence);
        let stored = usize::from(self.stream.push_sample(&decoded.values)?);
        Ok(StreamIngestReport {
            sequence,
            samples_received: 1,
            samples_stored: stored,
            lost_frames_total: self.sequence.lost_total(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nmixx_core::protocol::SequenceStatus;

    use super::*;
    use crate::StreamConfig;

    fn stream(channels: usize) -> StreamSession {
        StreamSession::new(StreamConfig {
            sample_rate_hz: 1000,
            channel_count: channels,
            history: Duration::from_secs(1),
        })
        .unwrap()
    }

    #[test]
    fn fast_frames_are_dequantized_and_written_to_ring() {
        let mut runtime = StreamPipeline::new(
            7,
            StreamWireMode::Fast {
                scales: vec![0.1, 0.5],
            },
            stream(2),
        )
        .unwrap();
        runtime.stream_mut().live();

        let frame = CanFdFrame::new(
            0x0601,
            &[1, 0, 7, 2, 10, 0, 236, 255, 20, 0, 216, 255],
        )
        .unwrap();
        let report = runtime.ingest_fast(&frame).unwrap();
        assert_eq!(report.sequence, SequenceStatus::First);
        assert_eq!(report.samples_received, 2);
        assert_eq!(report.samples_stored, 2);
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.sample(0), Some(&[1.0, -10.0][..]));
        assert_eq!(snapshot.sample(1), Some(&[2.0, -20.0][..]));
    }

    #[test]
    fn normal_gap_is_reported_but_data_is_still_stored() {
        let mut runtime = StreamPipeline::new(9, StreamWireMode::Normal, stream(2)).unwrap();
        runtime.stream_mut().live();

        for sequence in [1u16, 3] {
            let mut payload = vec![sequence as u8, (sequence >> 8) as u8, 9, 2];
            payload.extend_from_slice(&1.0f32.to_le_bytes());
            payload.extend_from_slice(&2.0f32.to_le_bytes());
            let frame = CanFdFrame::new(0x0401, &payload).unwrap();
            let report = runtime.ingest_normal(&frame).unwrap();
            if sequence == 3 {
                assert_eq!(
                    report.sequence,
                    SequenceStatus::Gap {
                        expected: 2,
                        actual: 3,
                        lost: 1,
                    }
                );
                assert_eq!(report.lost_frames_total, 1);
            }
        }
        assert_eq!(runtime.snapshot().sample_count(), 2);
    }

    #[test]
    fn paused_stream_decodes_but_does_not_store_samples() {
        let mut runtime = StreamPipeline::new(9, StreamWireMode::Normal, stream(1)).unwrap();
        runtime.stream_mut().pause();
        let mut payload = vec![1, 0, 9, 1];
        payload.extend_from_slice(&1.5f32.to_le_bytes());
        let frame = CanFdFrame::new(0x0401, &payload).unwrap();
        let report = runtime.ingest_normal(&frame).unwrap();
        assert_eq!(report.samples_received, 1);
        assert_eq!(report.samples_stored, 0);
        assert!(runtime.snapshot().is_empty());
    }
}
