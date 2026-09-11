use thiserror::Error;

use crate::wire::CanFdFrame;

use super::{
    AxdrStatus, MSG_PLOT, NODE_ID_DEFAULT, PLOT_CONFIG, PLOT_START, PLOT_STOP, ResponseFrame,
    TransactionId, can_id,
};

pub const PLOT_GROUP_FAST: u8 = 0;
pub const PLOT_GROUP_NORMAL: u8 = 1;
pub const PLOT_FAST_MASK: u8 = 1 << PLOT_GROUP_FAST;
pub const PLOT_NORMAL_MASK: u8 = 1 << PLOT_GROUP_NORMAL;

#[derive(Debug, Error, PartialEq, Eq)]
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
    fn builds_fast_config_and_group_controls() {
        let mut txns = TransactionTable::default();
        let txn = txns.allocate(MSG_PLOT, PLOT_CONFIG).unwrap();
        let frame = build_plot_config_default(txn, PLOT_GROUP_FAST, 7, &[0x0001, 0x0011]).unwrap();
        assert_eq!(frame.data()[..9], [1, PLOT_CONFIG, 0, 7, 2, 1, 0, 0x11, 0]);

        let start = build_plot_start_default(txn, PLOT_FAST_MASK).unwrap();
        assert_eq!(start.data()[..3], [1, PLOT_START, PLOT_FAST_MASK]);
        let stop = build_plot_stop_default(txn, PLOT_FAST_MASK | PLOT_NORMAL_MASK).unwrap();
        assert_eq!(stop.data()[..3], [1, PLOT_STOP, 3]);
    }
}
