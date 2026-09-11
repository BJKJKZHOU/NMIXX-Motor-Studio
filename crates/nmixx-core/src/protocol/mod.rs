mod action;
mod ids;
mod parameter;
mod plot;
mod router;
mod status;
mod stream;
mod transaction;

pub use action::{
    ActionError, ActionHandle, ActionState, ActionTracker, PARAM_ACTION_TYPE,
    build_action_start, build_action_start_default, parse_action_accept,
};
pub use ids::*;
pub use parameter::{
    ParameterError, ParameterType, ParameterValue, PositionValue,
    build_parameter_read, build_parameter_read_default, build_parameter_write,
    parse_parameter_read, parse_parameter_write,
};
pub use plot::{
    PLOT_FAST_MASK, PLOT_GROUP_FAST, PLOT_GROUP_NORMAL, PLOT_NORMAL_MASK, PlotError,
    build_plot_config, build_plot_config_default, build_plot_start, build_plot_start_default,
    build_plot_stop, build_plot_stop_default, parse_plot_config_response,
    parse_plot_start_response, parse_plot_stop_response,
};
pub use router::{
    ActionCompleteFrame, DecodeError, InboundFrame, ResponseFrame, decode_inbound,
};
pub use status::AxdrStatus;
pub use stream::{
    FastDataFrame, NormalDataFrame, SequenceStatus, SequenceTracker, StreamDecodeError,
    decode_fast_data, decode_normal_data,
};
pub use transaction::{
    PendingRequest, TransactionError, TransactionId, TransactionTable,
};

pub const NODE_ID_DEFAULT: u8 = 1;

pub fn can_id(message_type: u8, node_id: u8) -> u16 {
    ((message_type as u16) << 6) | (node_id as u16 & 0x3f)
}

pub fn split_can_id(id: u16) -> (u8, u8) {
    (((id >> 6) & 0x1f) as u8, (id & 0x3f) as u8)
}
