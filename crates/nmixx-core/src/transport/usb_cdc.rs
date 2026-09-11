use std::collections::VecDeque;
use std::io::ErrorKind;
use std::path::Path;
use std::time::{Duration, Instant};

use serial2::SerialPort;

use crate::wire::{CanFdFrame, UsbEnvelopeDecoder, encode_usb_envelope};

use super::{FrameTransport, TransportError};

const RX_CHUNK: usize = 512;

/// USB CDC transport for the current AXDR byte-stream envelope.
///
/// The serial port is transport-only: it does not know Parameter, Action,
/// Event or Stream semantics. Received bytes are converted into canonical
/// `CanFdFrame` values before crossing the `FrameTransport` boundary.
pub struct UsbCdcTransport {
    port: SerialPort,
    decoder: UsbEnvelopeDecoder,
    pending: VecDeque<CanFdFrame>,
}

impl UsbCdcTransport {
    pub fn open(path: impl AsRef<Path>, baud_rate: u32) -> Result<Self, TransportError> {
        let port = SerialPort::open(path, baud_rate)
            .map_err(|error| TransportError::Io(error.to_string()))?;

        Ok(Self {
            port,
            decoder: UsbEnvelopeDecoder::default(),
            pending: VecDeque::new(),
        })
    }

    pub fn available_ports() -> Result<Vec<String>, TransportError> {
        SerialPort::available_ports()
            .map(|ports| {
                ports
                    .into_iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect()
            })
            .map_err(|error| TransportError::Io(error.to_string()))
    }

    fn read_into_pending(&mut self, timeout: Duration) -> Result<(), TransportError> {
        self.port
            .set_read_timeout(timeout)
            .map_err(|error| TransportError::Io(error.to_string()))?;

        let mut buf = [0u8; RX_CHUNK];
        let count = match self.port.read(&mut buf) {
            Ok(count) => count,
            Err(error) if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => {
                return Err(TransportError::Timeout);
            }
            Err(error) => return Err(TransportError::Io(error.to_string())),
        };

        for frame in self.decoder.push(&buf[..count]) {
            match frame {
                Ok(frame) => self.pending.push_back(frame),
                Err(error) => return Err(TransportError::InvalidFrame(error.to_string())),
            }
        }

        Ok(())
    }
}

impl FrameTransport for UsbCdcTransport {
    fn send(&mut self, frame: &CanFdFrame) -> Result<(), TransportError> {
        self.port
            .write_all(&encode_usb_envelope(frame))
            .map_err(|error| TransportError::Io(error.to_string()))
    }

    fn receive(&mut self, timeout: Duration) -> Result<CanFdFrame, TransportError> {
        if let Some(frame) = self.pending.pop_front() {
            return Ok(frame);
        }

        let deadline = Instant::now() + timeout;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(TransportError::Timeout);
            }

            match self.read_into_pending(deadline.saturating_duration_since(now)) {
                Ok(()) => {
                    if let Some(frame) = self.pending.pop_front() {
                        return Ok(frame);
                    }
                }
                Err(TransportError::Timeout) => return Err(TransportError::Timeout),
                Err(error) => return Err(error),
            }
        }
    }
}
