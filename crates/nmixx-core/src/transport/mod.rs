use std::time::Duration;

use thiserror::Error;

use crate::wire::CanFdFrame;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("transport is not connected")]
    NotConnected,
    #[error("transport receive timed out")]
    Timeout,
    #[error("transport I/O failed: {0}")]
    Io(String),
    #[error("transport received an invalid frame: {0}")]
    InvalidFrame(String),
}

/// Transport boundary used by the device protocol.
///
/// Implementations own transport-specific details such as USB CDC byte-stream
/// framing or native CAN FD handles. Protocol code above this trait exchanges
/// only canonical `CanFdFrame` values.
pub trait FrameTransport: Send {
    fn send(&mut self, frame: &CanFdFrame) -> Result<(), TransportError>;

    fn receive(&mut self, timeout: Duration) -> Result<CanFdFrame, TransportError>;
}
