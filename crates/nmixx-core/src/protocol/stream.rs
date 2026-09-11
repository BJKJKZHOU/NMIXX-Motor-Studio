use thiserror::Error;

use crate::wire::CanFdFrame;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StreamDecodeError {
    #[error("stream frame payload is too short")]
    TooShort,
    #[error("FAST stream channel count must be in 1..=8, got {0}")]
    InvalidFastChannelCount(usize),
    #[error("stream payload requires {required} bytes but CAN FD frame carries only {actual}")]
    Truncated { required: usize, actual: usize },
    #[error("stream CAN FD padding is not zero")]
    NonZeroPadding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FastDataFrame {
    pub sequence: u16,
    pub config_id: u8,
    pub sample_count: u8,
    pub channel_count: u8,
    /// Sample-major, channel-minor signed quantized values.
    pub samples: Vec<i16>,
}

impl FastDataFrame {
    pub fn sample(&self, sample: usize, channel: usize) -> Option<i16> {
        if sample >= self.sample_count as usize || channel >= self.channel_count as usize {
            return None;
        }
        self.samples
            .get(sample * self.channel_count as usize + channel)
            .copied()
    }

    pub fn dequantize(&self, scales: &[f32]) -> Option<Vec<f32>> {
        if scales.len() != self.channel_count as usize {
            return None;
        }
        let mut values = Vec::with_capacity(self.samples.len());
        for sample in self.samples.chunks_exact(self.channel_count as usize) {
            for (raw, scale) in sample.iter().zip(scales) {
                values.push(*raw as f32 * *scale);
            }
        }
        Some(values)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalDataFrame {
    pub sequence: u16,
    pub config_id: u8,
    pub values: Vec<f32>,
}

pub fn decode_fast_data(
    frame: &CanFdFrame,
    channel_count: usize,
) -> Result<FastDataFrame, StreamDecodeError> {
    if !(1..=8).contains(&channel_count) {
        return Err(StreamDecodeError::InvalidFastChannelCount(channel_count));
    }

    let data = frame.data();
    if data.len() < 4 {
        return Err(StreamDecodeError::TooShort);
    }

    let sequence = u16::from_le_bytes([data[0], data[1]]);
    let config_id = data[2];
    let sample_count = data[3] as usize;
    let required = 4 + sample_count * channel_count * 2;
    validate_length_and_padding(data, required)?;

    let mut samples = Vec::with_capacity(sample_count * channel_count);
    for raw in data[4..required].chunks_exact(2) {
        samples.push(i16::from_le_bytes([raw[0], raw[1]]));
    }

    Ok(FastDataFrame {
        sequence,
        config_id,
        sample_count: sample_count as u8,
        channel_count: channel_count as u8,
        samples,
    })
}

pub fn decode_normal_data(frame: &CanFdFrame) -> Result<NormalDataFrame, StreamDecodeError> {
    let data = frame.data();
    if data.len() < 4 {
        return Err(StreamDecodeError::TooShort);
    }

    let sequence = u16::from_le_bytes([data[0], data[1]]);
    let config_id = data[2];
    let count = data[3] as usize;
    let required = 4 + count * 4;
    validate_length_and_padding(data, required)?;

    let mut values = Vec::with_capacity(count);
    for raw in data[4..required].chunks_exact(4) {
        values.push(f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]));
    }

    Ok(NormalDataFrame {
        sequence,
        config_id,
        values,
    })
}

fn validate_length_and_padding(data: &[u8], required: usize) -> Result<(), StreamDecodeError> {
    if required > data.len() {
        return Err(StreamDecodeError::Truncated {
            required,
            actual: data.len(),
        });
    }
    if data[required..].iter().any(|byte| *byte != 0) {
        return Err(StreamDecodeError::NonZeroPadding);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceStatus {
    First,
    Continuous,
    Gap {
        expected: u16,
        actual: u16,
        lost: u16,
    },
    ResetOrReorder {
        expected: u16,
        actual: u16,
    },
}

#[derive(Debug, Default, Clone)]
pub struct SequenceTracker {
    expected: Option<u16>,
    lost_total: u64,
}

impl SequenceTracker {
    pub fn observe(&mut self, sequence: u16) -> SequenceStatus {
        let Some(expected) = self.expected else {
            self.expected = Some(sequence.wrapping_add(1));
            return SequenceStatus::First;
        };

        let status = if sequence == expected {
            SequenceStatus::Continuous
        } else {
            let delta = sequence.wrapping_sub(expected);
            if delta < 0x8000 {
                self.lost_total += delta as u64;
                SequenceStatus::Gap {
                    expected,
                    actual: sequence,
                    lost: delta,
                }
            } else {
                SequenceStatus::ResetOrReorder {
                    expected,
                    actual: sequence,
                }
            }
        };

        self.expected = Some(sequence.wrapping_add(1));
        status
    }

    pub fn lost_total(&self) -> u64 {
        self.lost_total
    }

    pub fn reset(&mut self) {
        self.expected = None;
        self.lost_total = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_fast_sample_major_payload_and_scale() {
        let frame = CanFdFrame::new(
            0x0601,
            &[1, 0, 7, 2, 10, 0, 236, 255, 20, 0, 216, 255],
        )
        .unwrap();
        let decoded = decode_fast_data(&frame, 2).unwrap();
        assert_eq!(decoded.sequence, 1);
        assert_eq!(decoded.config_id, 7);
        assert_eq!(decoded.sample_count, 2);
        assert_eq!(decoded.samples, vec![10, -20, 20, -40]);
        assert_eq!(decoded.sample(1, 0), Some(20));
        assert_eq!(decoded.dequantize(&[0.1, 0.5]).unwrap(), vec![1.0, -10.0, 2.0, -20.0]);
    }

    #[test]
    fn decodes_normal_float_payload() {
        let mut payload = vec![2, 0, 9, 2];
        payload.extend_from_slice(&1.25f32.to_le_bytes());
        payload.extend_from_slice(&(-3.5f32).to_le_bytes());
        let frame = CanFdFrame::new(0x0401, &payload).unwrap();
        let decoded = decode_normal_data(&frame).unwrap();
        assert_eq!(decoded.sequence, 2);
        assert_eq!(decoded.config_id, 9);
        assert_eq!(decoded.values, vec![1.25, -3.5]);
    }

    #[test]
    fn tracks_gap_wrap_and_reset_or_reorder() {
        let mut tracker = SequenceTracker::default();
        assert_eq!(tracker.observe(65534), SequenceStatus::First);
        assert_eq!(tracker.observe(65535), SequenceStatus::Continuous);
        assert_eq!(tracker.observe(1), SequenceStatus::Gap { expected: 0, actual: 1, lost: 1 });
        assert_eq!(tracker.lost_total(), 1);
        assert_eq!(tracker.observe(0), SequenceStatus::ResetOrReorder { expected: 2, actual: 0 });
    }
}
