use std::collections::VecDeque;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
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
                let mut ports = ports
                    .into_iter()
                    .filter(|path| is_usb_serial_path(path))
                    .map(|path| display_usb_serial_path(&path).to_string_lossy().into_owned())
                    .collect::<Vec<_>>();
                ports.sort();
                ports.dedup();
                ports
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

impl Drop for UsbCdcTransport {
    fn drop(&mut self) {
        // A disconnected or non-responsive CDC device can leave bytes queued
        // in the tty output buffer. Linux may otherwise wait for that queue
        // while closing the file descriptor, delaying connection failure.
        let _ = self.port.discard_output_buffer();
    }
}

#[cfg(target_os = "linux")]
fn is_usb_serial_path(path: &Path) -> bool {
    let path = path.to_string_lossy();
    path.starts_with("/dev/ttyACM") || path.starts_with("/dev/ttyUSB")
}

#[cfg(target_os = "linux")]
fn display_usb_serial_path(path: &Path) -> PathBuf {
    display_usb_serial_path_in(path, Path::new("/dev/serial/by-id"))
}

#[cfg(target_os = "linux")]
fn display_usb_serial_path_in(path: &Path, by_id_dir: &Path) -> PathBuf {
    let Ok(target) = std::fs::canonicalize(path) else {
        return path.to_path_buf();
    };
    let Ok(entries) = std::fs::read_dir(by_id_dir) else {
        return path.to_path_buf();
    };

    let mut matches = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|candidate| {
            std::fs::canonicalize(candidate).is_ok_and(|resolved| resolved == target)
        })
        .collect::<Vec<_>>();
    matches.sort();
    matches
        .into_iter()
        .next()
        .unwrap_or_else(|| path.to_path_buf())
}

#[cfg(target_os = "macos")]
fn is_usb_serial_path(path: &Path) -> bool {
    let path = path.to_string_lossy();
    path.starts_with("/dev/cu.usb") || path.starts_with("/dev/tty.usb")
}

#[cfg(not(target_os = "linux"))]
fn display_usb_serial_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(any(target_os = "windows", not(any(target_os = "linux", target_os = "macos", target_os = "windows"))))]
fn is_usb_serial_path(_path: &Path) -> bool {
    true
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::display_usb_serial_path_in;

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "nmixx-usb-cdc-{}-{sequence}",
            std::process::id()
        ))
    }

    #[test]
    fn linux_port_list_prefers_stable_by_id_symlink() {
        let root = temp_dir();
        let dev_dir = root.join("dev");
        let by_id_dir = dev_dir.join("serial/by-id");
        let tty_path = dev_dir.join("ttyACM1");
        let stable_path = by_id_dir.join("usb-STM32_000000000001-if00");

        fs::create_dir_all(&by_id_dir).unwrap();
        fs::write(&tty_path, []).unwrap();
        symlink("../../ttyACM1", &stable_path).unwrap();

        assert_eq!(
            display_usb_serial_path_in(&tty_path, &by_id_dir),
            stable_path
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn linux_port_list_falls_back_to_tty_without_by_id_match() {
        let root = temp_dir();
        let tty_path = root.join("ttyACM0");
        fs::create_dir_all(&root).unwrap();
        fs::write(&tty_path, []).unwrap();

        assert_eq!(
            display_usb_serial_path_in(&tty_path, &root.join("missing")),
            tty_path
        );

        fs::remove_dir_all(root).unwrap();
    }
}
