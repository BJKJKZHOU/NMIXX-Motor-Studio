pub const MSG_RESPONSE: u8 = 0x02;
pub const MSG_PLOT: u8 = 0x04;
pub const MSG_PARAMETER: u8 = 0x07;
pub const MSG_EVENT: u8 = 0x08;
pub const MSG_NORMAL_DATA: u8 = 0x10;
pub const MSG_FAST_DATA: u8 = 0x18;

pub const PARAM_READ: u8 = 0x01;
pub const PARAM_WRITE: u8 = 0x02;

pub const EVENT_NOTIFY: u8 = 0x01;
pub const EVENT_ACTION_COMPLETE: u8 = 0x03;

pub const PLOT_CONFIG: u8 = 0x01;
pub const PLOT_START: u8 = 0x02;
pub const PLOT_STOP: u8 = 0x03;
