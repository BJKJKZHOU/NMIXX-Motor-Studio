use thiserror::Error;

pub const MAX_CANFD_DATA_LEN: usize = 64;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CanFdError {
    #[error("CAN identifier 0x{0:04X} exceeds 11-bit standard identifier range")]
    InvalidStandardId(u16),
    #[error("CAN FD payload length {0} exceeds 64 bytes")]
    PayloadTooLong(usize),
    #[error("CAN FD data-field length {0} is not canonical")]
    NonCanonicalLength(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanFdFrame {
    pub id: u16,
    data: Vec<u8>,
}

impl CanFdFrame {
    pub fn new(id: u16, payload: &[u8]) -> Result<Self, CanFdError> {
        if id > 0x07ff {
            return Err(CanFdError::InvalidStandardId(id));
        }

        let len = canonical_data_len(payload.len())?;
        let mut data = vec![0u8; len];
        data[..payload.len()].copy_from_slice(payload);
        Ok(Self { id, data })
    }

    pub fn from_canonical_data(id: u16, data: Vec<u8>) -> Result<Self, CanFdError> {
        if id > 0x07ff {
            return Err(CanFdError::InvalidStandardId(id));
        }
        if !is_canonical_data_len(data.len()) {
            return Err(CanFdError::NonCanonicalLength(data.len()));
        }
        Ok(Self { id, data })
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_len(&self) -> usize {
        self.data.len()
    }
}

pub fn canonical_data_len(required: usize) -> Result<usize, CanFdError> {
    if required > MAX_CANFD_DATA_LEN {
        return Err(CanFdError::PayloadTooLong(required));
    }

    let len = match required {
        0..=8 => required,
        9..=12 => 12,
        13..=16 => 16,
        17..=20 => 20,
        21..=24 => 24,
        25..=32 => 32,
        33..=48 => 48,
        49..=MAX_CANFD_DATA_LEN => MAX_CANFD_DATA_LEN,
        _ => unreachable!("payload length was validated against CAN FD maximum"),
    };
    Ok(len)
}

pub fn is_canonical_data_len(len: usize) -> bool {
    matches!(len, 0..=8 | 12 | 16 | 20 | 24 | 32 | 48 | MAX_CANFD_DATA_LEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_lengths_match_axdr_firmware() {
        assert_eq!(canonical_data_len(0), Ok(0));
        assert_eq!(canonical_data_len(8), Ok(8));
        assert_eq!(canonical_data_len(9), Ok(12));
        assert_eq!(canonical_data_len(19), Ok(20));
        assert_eq!(canonical_data_len(33), Ok(48));
        assert_eq!(canonical_data_len(MAX_CANFD_DATA_LEN), Ok(MAX_CANFD_DATA_LEN));
        assert!(canonical_data_len(MAX_CANFD_DATA_LEN + 1).is_err());
    }

    #[test]
    fn frame_zero_pads_to_canonical_length() {
        let frame = CanFdFrame::new(0x01c1, &[1; 19]).unwrap();
        assert_eq!(frame.data_len(), 20);
        assert_eq!(&frame.data()[..19], &[1; 19]);
        assert_eq!(frame.data()[19], 0);
    }
}
