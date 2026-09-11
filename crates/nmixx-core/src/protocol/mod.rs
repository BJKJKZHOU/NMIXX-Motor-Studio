mod ids;
mod status;

pub use ids::*;
pub use status::AxdrStatus;

pub const NODE_ID_DEFAULT: u8 = 1;

pub fn can_id(message_type: u8, node_id: u8) -> u16 {
    ((message_type as u16) << 6) | (node_id as u16 & 0x3f)
}

pub fn split_can_id(id: u16) -> (u8, u8) {
    (((id >> 6) & 0x1f) as u8, (id & 0x3f) as u8)
}
