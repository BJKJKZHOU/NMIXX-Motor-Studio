use std::path::Path;

use nmixx_core::transport::UsbCdcTransport;

use crate::{DeviceSession, SessionError};

/// USB CDC line rate. CDC ACM devices usually ignore this value, but the host
/// serial API requires one and keeping it explicit makes CLI behavior stable.
pub const DEFAULT_USB_BAUD: u32 = 115_200;

impl DeviceSession {
    pub fn open_usb(path: impl AsRef<Path>, baud_rate: u32) -> Result<Self, SessionError> {
        let transport = UsbCdcTransport::open(path, baud_rate)?;
        Ok(Self::spawn(Box::new(transport)))
    }

    pub fn available_usb_ports() -> Result<Vec<String>, SessionError> {
        Ok(UsbCdcTransport::available_ports()?)
    }
}
