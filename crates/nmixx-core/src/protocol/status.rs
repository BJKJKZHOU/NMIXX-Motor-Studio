#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AxdrStatus {
    Ok = 0,
    ErrOp = 1,
    ErrLength = 2,
    ErrVarId = 3,
    ErrReadOnly = 4,
    ErrValue = 5,
    ErrState = 6,
    ErrConfig = 7,
    ErrBandwidth = 8,
    ErrNotSupported = 9,
}

impl TryFrom<u8> for AxdrStatus {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::Ok,
            1 => Self::ErrOp,
            2 => Self::ErrLength,
            3 => Self::ErrVarId,
            4 => Self::ErrReadOnly,
            5 => Self::ErrValue,
            6 => Self::ErrState,
            7 => Self::ErrConfig,
            8 => Self::ErrBandwidth,
            9 => Self::ErrNotSupported,
            other => return Err(other),
        })
    }
}
