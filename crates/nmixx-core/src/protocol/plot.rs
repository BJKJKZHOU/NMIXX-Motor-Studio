use thiserror::Error;

use crate::wire::CanFdFrame;

use super::{
    AxdrStatus, MSG_PLOT, NODE_ID_DEFAULT, PLOT_CAPS, PLOT_CONFIG, PLOT_START, PLOT_STOP,
    ResponseFrame, TransactionId, can_id,
};

pub const PLOT_GROUP_FAST: u8 = 0;
pub const PLOT_GROUP_NORMAL: u8 = 1;
pub const PLOT_FAST_MASK: u8 = 1 << PLOT_GROUP_FAST;
pub const PLOT_NORMAL_MASK: u8 = 1 << PLOT_GROUP_NORMAL;

pub const PLOT_CAP_FAST: u8 = 1 << 0;
pub const PLOT_CAP_NORMAL: u8 = 1 << 1;
pub const PLOT_CAP_END: u8 = 0xff;
pub const PLOT_CAP_HEADER_LEN: usize = 14;
pub const PLOT_CAP_ENTRY_LEN: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotCapabilityEntry {
    pub id: u16,
    pub modes: u8,
    pub fast_scale: f32,
}

impl PlotCapabilityEntry {
    pub fn supports_fast(self) -> bool {
        self.modes & PLOT_CAP_FAST != 0
    }

    pub fn supports_normal(self) -> bool {
        self.modes & PLOT_CAP_NORMAL != 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlotCapabilitiesPage {
    pub start: u8,
    pub next: Option<u8>,
    pub fast_max_channels: u8,
    pub normal_max_channels: u8,
    pub fast_block_samples: u8,
    pub fast_rate_hz: u32,
    pub normal_rate_hz: u32,
    pub entries: Vec<PlotCapabilityEntry>,
}

#[derive(Debug, Error, PartialEq)]
pub enum PlotError {
    #[error("FAST Plot supports at most 8 channels, got {0}")]
    TooManyFastChannels(usize),
    #[error("NORMAL Plot supports at most 15 channels, got {0}")]
    TooManyNormalChannels(usize),
    #[error("Plot channel list must not be empty")]
    EmptyChannels,
    #[error("invalid Plot group {0}")]
    InvalidGroup(u8),
    #[error("invalid Plot group mask 0x{0:02X}")]
    InvalidGroupMask(u8),
    #[error("unexpected Plot response header")]
    UnexpectedResponse,
    #[error("Plot request failed with status {0:?}")]
    Status(AxdrStatus),
    #[error("Plot CONFIG response is malformed")]
    MalformedConfigResponse,
    #[error("Plot CAPS response is malformed")]
    MalformedCapabilitiesResponse,
    #[error("Plot CAPS page starts at {actual}, expected {expected}")]
    CapabilityStartMismatch { expected: u8, actual: u8 },
    #[error("Plot CAPS entry 0x{id:04X} has invalid mode mask 0x{modes:02X}")]
    InvalidCapabilityModes { id: u16, modes: u8 },
    #[error("Plot CAPS FAST entry 0x{id:04X} has invalid scale {scale}")]
    InvalidCapabilityScale { id: u16, scale: f32 },
}

pub fn build_plot_config_default(
    txn: TransactionId,
    group: u8,
    config_id: u8,
    parameters: &[u16],
) -> Result<CanFdFrame, PlotError> {
    build_plot_config(txn, group, config_id, parameters, NODE_ID_DEFAULT)
}

pub fn build_plot_config(
    txn: TransactionId,
    group: u8,
    config_id: u8,
    parameters: &[u16],
    node_id: u8,
) -> Result<CanFdFrame, PlotError> {
    if parameters.is_empty() {
        return Err(PlotError::EmptyChannels);
    }
    match group {
        PLOT_GROUP_FAST if parameters.len() > 8 => {
            return Err(PlotError::TooManyFastChannels(parameters.len()));
        }
        PLOT_GROUP_NORMAL if parameters.len() > 15 => {
            return Err(PlotError::TooManyNormalChannels(parameters.len()));
        }
        PLOT_GROUP_FAST | PLOT_GROUP_NORMAL => {}
        other => return Err(PlotError::InvalidGroup(other)),
    }

    let mut payload = Vec::with_capacity(5 + parameters.len() * 2);
    payload.push(txn.get());
    payload.push(PLOT_CONFIG);
    payload.push(group);
    payload.push(config_id);
    payload.push(parameters.len() as u8);
    for parameter in parameters {
        payload.extend_from_slice(&parameter.to_le_bytes());
    }
    CanFdFrame::new(can_id(MSG_PLOT, node_id), &payload)
        .map_err(|_| PlotError::MalformedConfigResponse)
}

pub fn build_plot_start_default(txn: TransactionId, group_mask: u8) -> Result<CanFdFrame, PlotError> {
    build_plot_start(txn, group_mask, NODE_ID_DEFAULT)
}

pub fn build_plot_start(
    txn: TransactionId,
    group_mask: u8,
    node_id: u8,
) -> Result<CanFdFrame, PlotError> {
    validate_group_mask(group_mask)?;
    CanFdFrame::new(
        can_id(MSG_PLOT, node_id),
        &[txn.get(), PLOT_START, group_mask],
    )
    .map_err(|_| PlotError::UnexpectedResponse)
}

pub fn build_plot_stop_default(txn: TransactionId, group_mask: u8) -> Result<CanFdFrame, PlotError> {
    build_plot_stop(txn, group_mask, NODE_ID_DEFAULT)
}

pub fn build_plot_stop(
    txn: TransactionId,
    group_mask: u8,
    node_id: u8,
) -> Result<CanFdFrame, PlotError> {
    validate_group_mask(group_mask)?;
    CanFdFrame::new(
        can_id(MSG_PLOT, node_id),
        &[txn.get(), PLOT_STOP, group_mask],
    )
    .map_err(|_| PlotError::UnexpectedResponse)
}

pub fn build_plot_caps_default(txn: TransactionId, start: u8) -> Result<CanFdFrame, PlotError> {
    build_plot_caps(txn, start, NODE_ID_DEFAULT)
}

pub fn build_plot_caps(
    txn: TransactionId,
    start: u8,
    node_id: u8,
) -> Result<CanFdFrame, PlotError> {
    CanFdFrame::new(can_id(MSG_PLOT, node_id), &[txn.get(), PLOT_CAPS, start])
        .map_err(|_| PlotError::MalformedCapabilitiesResponse)
}

fn validate_group_mask(group_mask: u8) -> Result<(), PlotError> {
    let valid = PLOT_FAST_MASK | PLOT_NORMAL_MASK;
    if group_mask == 0 || group_mask & !valid != 0 {
        return Err(PlotError::InvalidGroupMask(group_mask));
    }
    Ok(())
}

pub fn parse_plot_config_response(
    response: &ResponseFrame,
    group: u8,
    config_id: u8,
    parameters: &[u16],
) -> Result<(), PlotError> {
    parse_status(response, PLOT_CONFIG)?;
    if response.data.len() < 3 {
        return Err(PlotError::MalformedConfigResponse);
    }
    if response.data[0] != group
        || response.data[1] != config_id
        || response.data[2] as usize != parameters.len()
    {
        return Err(PlotError::MalformedConfigResponse);
    }
    let required = 3 + parameters.len() * 2;
    if response.data.len() < required {
        return Err(PlotError::MalformedConfigResponse);
    }
    for (index, parameter) in parameters.iter().enumerate() {
        let offset = 3 + index * 2;
        let actual = u16::from_le_bytes([response.data[offset], response.data[offset + 1]]);
        if actual != *parameter {
            return Err(PlotError::MalformedConfigResponse);
        }
    }
    Ok(())
}

pub fn parse_plot_start_response(response: &ResponseFrame) -> Result<(), PlotError> {
    parse_status(response, PLOT_START)
}

pub fn parse_plot_stop_response(response: &ResponseFrame) -> Result<(), PlotError> {
    parse_status(response, PLOT_STOP)
}

pub fn parse_plot_caps_response(
    response: &ResponseFrame,
    expected_start: u8,
) -> Result<PlotCapabilitiesPage, PlotError> {
    parse_status(response, PLOT_CAPS)?;
    if response.data.len() < PLOT_CAP_HEADER_LEN {
        return Err(PlotError::MalformedCapabilitiesResponse);
    }

    let start = response.data[0];
    if start != expected_start {
        return Err(PlotError::CapabilityStartMismatch {
            expected: expected_start,
            actual: start,
        });
    }

    let next_raw = response.data[1];
    let count = response.data[2] as usize;
    let required = PLOT_CAP_HEADER_LEN + count * PLOT_CAP_ENTRY_LEN;
    if response.data.len() < required {
        return Err(PlotError::MalformedCapabilitiesResponse);
    }

    let fast_max_channels = response.data[3];
    let normal_max_channels = response.data[4];
    let fast_block_samples = response.data[5];
    let fast_rate_hz = u32::from_le_bytes([
        response.data[6],
        response.data[7],
        response.data[8],
        response.data[9],
    ]);
    let normal_rate_hz = u32::from_le_bytes([
        response.data[10],
        response.data[11],
        response.data[12],
        response.data[13],
    ]);

    if count == 0 || fast_max_channels == 0 || normal_max_channels == 0 || fast_block_samples == 0 {
        return Err(PlotError::MalformedCapabilitiesResponse);
    }

    let mut entries = Vec::with_capacity(count);
    let mut offset = PLOT_CAP_HEADER_LEN;
    for _ in 0..count {
        let id = u16::from_le_bytes([response.data[offset], response.data[offset + 1]]);
        let modes = response.data[offset + 2];
        if modes == 0 || modes & !(PLOT_CAP_FAST | PLOT_CAP_NORMAL) != 0 {
            return Err(PlotError::InvalidCapabilityModes { id, modes });
        }
        let fast_scale = f32::from_le_bytes([
            response.data[offset + 3],
            response.data[offset + 4],
            response.data[offset + 5],
            response.data[offset + 6],
        ]);
        if modes & PLOT_CAP_FAST != 0 && (!fast_scale.is_finite() || fast_scale <= 0.0) {
            return Err(PlotError::InvalidCapabilityScale { id, scale: fast_scale });
        }
        entries.push(PlotCapabilityEntry {
            id,
            modes,
            fast_scale,
        });
        offset += PLOT_CAP_ENTRY_LEN;
    }

    let next = if next_raw == PLOT_CAP_END {
        None
    } else {
        let expected_next = start
            .checked_add(count as u8)
            .ok_or(PlotError::MalformedCapabilitiesResponse)?;
        if next_raw != expected_next {
            return Err(PlotError::MalformedCapabilitiesResponse);
        }
        Some(next_raw)
    };

    Ok(PlotCapabilitiesPage {
        start,
        next,
        fast_max_channels,
        normal_max_channels,
        fast_block_samples,
        fast_rate_hz,
        normal_rate_hz,
        entries,
    })
}

fn parse_status(response: &ResponseFrame, op: u8) -> Result<(), PlotError> {
    if response.request_message != MSG_PLOT || response.request_op != op {
        return Err(PlotError::UnexpectedResponse);
    }
    if response.status != AxdrStatus::Ok {
        return Err(PlotError::Status(response.status));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::TransactionTable;

    #[test]
    fn builds_fast_config_group_controls_and_caps() {
        let mut txns = TransactionTable::default();
        let txn = txns.allocate(MSG_PLOT, PLOT_CONFIG).unwrap();
        let frame = build_plot_config_default(txn, PLOT_GROUP_FAST, 7, &[0x0001, 0x0011]).unwrap();
        assert_eq!(frame.data()[..9], [1, PLOT_CONFIG, 0, 7, 2, 1, 0, 0x11, 0]);

        let start = build_plot_start_default(txn, PLOT_FAST_MASK).unwrap();
        assert_eq!(start.data()[..3], [1, PLOT_START, PLOT_FAST_MASK]);
        let stop = build_plot_stop_default(txn, PLOT_FAST_MASK | PLOT_NORMAL_MASK).unwrap();
        assert_eq!(stop.data()[..3], [1, PLOT_STOP, 3]);
        let caps = build_plot_caps_default(txn, 6).unwrap();
        assert_eq!(caps.data()[..3], [1, PLOT_CAPS, 6]);
    }

    #[test]
    fn decodes_capability_page() {
        let mut data = vec![0, PLOT_CAP_END, 2, 8, 15, 20];
        data.extend_from_slice(&20_000u32.to_le_bytes());
        data.extend_from_slice(&1_000u32.to_le_bytes());
        data.extend_from_slice(&0x0001u16.to_le_bytes());
        data.push(PLOT_CAP_FAST | PLOT_CAP_NORMAL);
        data.extend_from_slice(&0.001f32.to_le_bytes());
        data.extend_from_slice(&0x0004u16.to_le_bytes());
        data.push(PLOT_CAP_NORMAL);
        data.extend_from_slice(&0.0f32.to_le_bytes());
        let response = ResponseFrame {
            txn: 1,
            request_message: MSG_PLOT,
            request_op: PLOT_CAPS,
            status: AxdrStatus::Ok,
            data,
        };

        let page = parse_plot_caps_response(&response, 0).unwrap();
        assert_eq!(page.next, None);
        assert_eq!(page.fast_max_channels, 8);
        assert_eq!(page.normal_max_channels, 15);
        assert_eq!(page.fast_block_samples, 20);
        assert_eq!(page.fast_rate_hz, 20_000);
        assert_eq!(page.normal_rate_hz, 1_000);
        assert!(page.entries[0].supports_fast());
        assert!(page.entries[0].supports_normal());
        assert!(!page.entries[1].supports_fast());
        assert!(page.entries[1].supports_normal());
    }
}
