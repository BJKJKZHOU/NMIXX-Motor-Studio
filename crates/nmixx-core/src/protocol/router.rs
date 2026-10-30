use thiserror::Error;

use crate::wire::CanFdFrame;

use super::{
    EVENT_NOTIFY, MSG_EVENT, MSG_FAST_DATA, MSG_NORMAL_DATA, MSG_RESPONSE, AxdrStatus, split_can_id,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("frame payload is too short for {0}")]
    TooShort(&'static str),
    #[error("unknown AXDR status {0}")]
    UnknownStatus(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseFrame {
    pub txn: u8,
    pub request_message: u8,
    pub request_op: u8,
    pub status: AxdrStatus,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCompleteFrame {
    pub txn: u8,
    pub action_id: u16,
    pub status: AxdrStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtectionEventFrame {
    pub report: u32,
    pub warning: u32,
    pub error: u32,
    pub trip: u32,
}

pub fn parse_protection_event(frame: &CanFdFrame) -> Result<Option<ProtectionEventFrame>, DecodeError> {
    let (message_type, _) = split_can_id(frame.id);
    let data = frame.data();
    if message_type != MSG_EVENT || data.first().copied() != Some(EVENT_NOTIFY) {
        return Ok(None);
    }
    if data.len() < 17 {
        return Err(DecodeError::TooShort("protection event"));
    }
    Ok(Some(ProtectionEventFrame {
        report: u32::from_le_bytes([data[1], data[2], data[3], data[4]]),
        warning: u32::from_le_bytes([data[5], data[6], data[7], data[8]]),
        error: u32::from_le_bytes([data[9], data[10], data[11], data[12]]),
        trip: u32::from_le_bytes([data[13], data[14], data[15], data[16]]),
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundFrame {
    Response(ResponseFrame),
    ActionComplete(ActionCompleteFrame),
    Event(CanFdFrame),
    FastData(CanFdFrame),
    NormalData(CanFdFrame),
    Unknown(CanFdFrame),
}

pub fn decode_inbound(frame: CanFdFrame) -> Result<InboundFrame, DecodeError> {
    let (message_type, _) = split_can_id(frame.id);
    let data = frame.data();

    match message_type {
        MSG_RESPONSE => {
            if data.len() < 4 {
                return Err(DecodeError::TooShort("response"));
            }
            let status = AxdrStatus::try_from(data[3])
                .map_err(DecodeError::UnknownStatus)?;
            Ok(InboundFrame::Response(ResponseFrame {
                txn: data[0],
                request_message: data[1],
                request_op: data[2],
                status,
                data: data[4..].to_vec(),
            }))
        }
        MSG_EVENT if data.first().copied() == Some(super::EVENT_ACTION_COMPLETE) => {
            if data.len() < 5 {
                return Err(DecodeError::TooShort("action-complete event"));
            }
            let status = AxdrStatus::try_from(data[4])
                .map_err(DecodeError::UnknownStatus)?;
            Ok(InboundFrame::ActionComplete(ActionCompleteFrame {
                txn: data[1],
                action_id: u16::from_le_bytes([data[2], data[3]]),
                status,
            }))
        }
        MSG_EVENT => Ok(InboundFrame::Event(frame)),
        MSG_FAST_DATA => Ok(InboundFrame::FastData(frame)),
        MSG_NORMAL_DATA => Ok(InboundFrame::NormalData(frame)),
        _ => Ok(InboundFrame::Unknown(frame)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{EVENT_ACTION_COMPLETE, NODE_ID_DEFAULT, can_id};

    #[test]
    fn decodes_response_header() {
        let frame = CanFdFrame::new(
            can_id(MSG_RESPONSE, NODE_ID_DEFAULT),
            &[7, 0x07, 0x01, 0, 0x10, 0x01],
        )
        .unwrap();

        let InboundFrame::Response(response) = decode_inbound(frame).unwrap() else {
            panic!("expected response");
        };
        assert_eq!(response.txn, 7);
        assert_eq!(response.request_message, 0x07);
        assert_eq!(response.request_op, 0x01);
        assert_eq!(response.status, AxdrStatus::Ok);
        assert_eq!(&response.data[..2], &[0x10, 0x01]);
    }

    #[test]
    fn parses_protection_event_without_changing_generic_event_routing() {
        let frame = CanFdFrame::new(
            can_id(MSG_EVENT, NODE_ID_DEFAULT),
            &[
                EVENT_NOTIFY,
                1, 0, 0, 0,
                2, 0, 0, 0,
                8, 0, 0, 0,
                16, 0, 0, 0,
            ],
        )
        .unwrap();

        let parsed = parse_protection_event(&frame).unwrap().unwrap();
        assert_eq!(parsed.report, 1);
        assert_eq!(parsed.warning, 2);
        assert_eq!(parsed.error, 8);
        assert_eq!(parsed.trip, 16);
        assert!(matches!(decode_inbound(frame).unwrap(), InboundFrame::Event(_)));
    }

    #[test]
    fn decodes_action_complete() {
        let frame = CanFdFrame::new(
            can_id(MSG_EVENT, NODE_ID_DEFAULT),
            &[EVENT_ACTION_COMPLETE, 9, 0x02, 0x11, 0],
        )
        .unwrap();

        let InboundFrame::ActionComplete(event) = decode_inbound(frame).unwrap() else {
            panic!("expected action completion");
        };
        assert_eq!(event.txn, 9);
        assert_eq!(event.action_id, 0x1102);
        assert_eq!(event.status, AxdrStatus::Ok);
    }
}
