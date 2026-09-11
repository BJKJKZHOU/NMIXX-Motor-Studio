use thiserror::Error;

use crate::wire::{CanFdError, CanFdFrame};

use super::{can_id, AxdrStatus, ResponseFrame, TransactionId, MSG_PARAMETER, NODE_ID_DEFAULT, PARAM_READ, PARAM_WRITE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ParameterType {
    U8 = 0,
    I8 = 1,
    F32 = 2,
    I32 = 3,
    U32 = 4,
    Position = 5,
    Action = 6,
}

impl TryFrom<u8> for ParameterType {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::U8,
            1 => Self::I8,
            2 => Self::F32,
            3 => Self::I32,
            4 => Self::U32,
            5 => Self::Position,
            6 => Self::Action,
            other => return Err(other),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionValue {
    pub turns: i32,
    pub theta: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParameterValue {
    U8(u8),
    I8(i8),
    F32(f32),
    I32(i32),
    U32(u32),
    Position(PositionValue),
}

impl ParameterValue {
    pub fn parameter_type(self) -> ParameterType {
        match self {
            Self::U8(_) => ParameterType::U8,
            Self::I8(_) => ParameterType::I8,
            Self::F32(_) => ParameterType::F32,
            Self::I32(_) => ParameterType::I32,
            Self::U32(_) => ParameterType::U32,
            Self::Position(_) => ParameterType::Position,
        }
    }

    fn encode(self, output: &mut Vec<u8>) {
        match self {
            Self::U8(v) => output.push(v),
            Self::I8(v) => output.push(v as u8),
            Self::F32(v) => output.extend_from_slice(&v.to_le_bytes()),
            Self::I32(v) => output.extend_from_slice(&v.to_le_bytes()),
            Self::U32(v) => output.extend_from_slice(&v.to_le_bytes()),
            Self::Position(v) => {
                output.extend_from_slice(&v.turns.to_le_bytes());
                output.extend_from_slice(&v.theta.to_le_bytes());
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum ParameterError {
    #[error(transparent)]
    Frame(#[from] CanFdError),
    #[error("parameter response has invalid payload length")]
    InvalidLength,
    #[error("parameter response ID mismatch: expected 0x{expected:04x}, got 0x{actual:04x}")]
    IdMismatch { expected: u16, actual: u16 },
    #[error("unknown parameter type {0}")]
    UnknownType(u8),
    #[error("parameter response type mismatch: expected {expected:?}, got {actual:?}")]
    TypeMismatch { expected: ParameterType, actual: ParameterType },
    #[error("parameter request failed with status {0:?}")]
    Status(AxdrStatus),
}

pub fn build_parameter_read(txn: TransactionId, parameter_id: u16, node_id: u8) -> Result<CanFdFrame, ParameterError> {
    let payload = [txn.get(), PARAM_READ, parameter_id as u8, (parameter_id >> 8) as u8];
    Ok(CanFdFrame::new(can_id(MSG_PARAMETER, node_id), &payload)?)
}

pub fn build_parameter_read_default(txn: TransactionId, parameter_id: u16) -> Result<CanFdFrame, ParameterError> {
    build_parameter_read(txn, parameter_id, NODE_ID_DEFAULT)
}

pub fn build_parameter_write(txn: TransactionId, parameter_id: u16, value: ParameterValue, node_id: u8) -> Result<CanFdFrame, ParameterError> {
    let mut payload = vec![txn.get(), PARAM_WRITE, parameter_id as u8, (parameter_id >> 8) as u8, value.parameter_type() as u8];
    value.encode(&mut payload);
    Ok(CanFdFrame::new(can_id(MSG_PARAMETER, node_id), &payload)?)
}

pub fn parse_parameter_read(response: &ResponseFrame, parameter_id: u16, expected_type: ParameterType) -> Result<ParameterValue, ParameterError> {
    if response.status != AxdrStatus::Ok {
        return Err(ParameterError::Status(response.status));
    }
    let data = &response.data;
    if data.len() < 3 {
        return Err(ParameterError::InvalidLength);
    }
    let actual_id = u16::from_le_bytes([data[0], data[1]]);
    if actual_id != parameter_id {
        return Err(ParameterError::IdMismatch { expected: parameter_id, actual: actual_id });
    }
    let actual_type = ParameterType::try_from(data[2]).map_err(ParameterError::UnknownType)?;
    if actual_type != expected_type {
        return Err(ParameterError::TypeMismatch { expected: expected_type, actual: actual_type });
    }
    decode_value(actual_type, &data[3..])
}

pub fn parse_parameter_write(response: &ResponseFrame, parameter_id: u16) -> Result<(), ParameterError> {
    if response.status != AxdrStatus::Ok {
        return Err(ParameterError::Status(response.status));
    }
    if response.data.len() < 2 {
        return Err(ParameterError::InvalidLength);
    }
    let actual_id = u16::from_le_bytes([response.data[0], response.data[1]]);
    if actual_id != parameter_id {
        return Err(ParameterError::IdMismatch { expected: parameter_id, actual: actual_id });
    }
    Ok(())
}

fn decode_value(ty: ParameterType, data: &[u8]) -> Result<ParameterValue, ParameterError> {
    Ok(match ty {
        ParameterType::U8 if !data.is_empty() => ParameterValue::U8(data[0]),
        ParameterType::I8 if !data.is_empty() => ParameterValue::I8(data[0] as i8),
        ParameterType::F32 if data.len() >= 4 => ParameterValue::F32(f32::from_le_bytes(data[..4].try_into().unwrap())),
        ParameterType::I32 if data.len() >= 4 => ParameterValue::I32(i32::from_le_bytes(data[..4].try_into().unwrap())),
        ParameterType::U32 if data.len() >= 4 => ParameterValue::U32(u32::from_le_bytes(data[..4].try_into().unwrap())),
        ParameterType::Position if data.len() >= 8 => ParameterValue::Position(PositionValue {
            turns: i32::from_le_bytes(data[..4].try_into().unwrap()),
            theta: f32::from_le_bytes(data[4..8].try_into().unwrap()),
        }),
        _ => return Err(ParameterError::InvalidLength),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_position_in_one_payload() {
        let frame = build_parameter_write(
            TransactionId::from_raw(7),
            0x0704,
            ParameterValue::Position(PositionValue { turns: -2, theta: 1.5 }),
            1,
        ).unwrap();
        assert_eq!(&frame.data()[0..5], &[7, PARAM_WRITE, 0x04, 0x07, ParameterType::Position as u8]);
    }
}
