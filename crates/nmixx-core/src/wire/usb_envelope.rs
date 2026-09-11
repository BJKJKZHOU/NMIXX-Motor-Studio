use thiserror::Error;

use super::{CanFdError, CanFdFrame, is_canonical_data_len};

pub const AXDR_MAGIC: [u8; 4] = *b"AXDR";
const HEADER_LEN: usize = 7;

#[derive(Debug, Error)]
pub enum UsbEnvelopeError {
    #[error(transparent)]
    CanFd(#[from] CanFdError),
}

pub fn encode_usb_envelope(frame: &CanFdFrame) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + frame.data_len());
    out.extend_from_slice(&AXDR_MAGIC);
    out.extend_from_slice(&frame.id.to_le_bytes());
    out.push(frame.data_len() as u8);
    out.extend_from_slice(frame.data());
    out
}

#[derive(Debug, Default)]
pub struct UsbEnvelopeDecoder {
    buffer: Vec<u8>,
}

impl UsbEnvelopeDecoder {
    pub fn push(&mut self, input: &[u8]) -> Vec<Result<CanFdFrame, UsbEnvelopeError>> {
        self.buffer.extend_from_slice(input);
        let mut frames = Vec::new();

        loop {
            let Some(pos) = find_magic(&self.buffer) else {
                if self.buffer.len() > AXDR_MAGIC.len() - 1 {
                    let keep = AXDR_MAGIC.len() - 1;
                    self.buffer.drain(..self.buffer.len() - keep);
                }
                break;
            };

            if pos != 0 {
                self.buffer.drain(..pos);
            }

            if self.buffer.len() < HEADER_LEN {
                break;
            }

            let id = u16::from_le_bytes([self.buffer[4], self.buffer[5]]);
            let len = self.buffer[6] as usize;

            if id > 0x07ff || !is_canonical_data_len(len) {
                self.buffer.drain(..1);
                continue;
            }

            let frame_len = HEADER_LEN + len;
            if self.buffer.len() < frame_len {
                break;
            }

            let data = self.buffer[HEADER_LEN..frame_len].to_vec();
            self.buffer.drain(..frame_len);
            frames.push(
                CanFdFrame::from_canonical_data(id, data)
                    .map_err(UsbEnvelopeError::from),
            );
        }

        frames
    }
}

fn find_magic(data: &[u8]) -> Option<usize> {
    data.windows(AXDR_MAGIC.len())
        .position(|window| window == AXDR_MAGIC)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_handles_fragmented_stream_and_noise() {
        let frame = CanFdFrame::new(0x01c1, &[1; 19]).unwrap();
        let encoded = encode_usb_envelope(&frame);

        let mut decoder = UsbEnvelopeDecoder::default();
        assert!(decoder.push(b"noiseAX").is_empty());
        let mut remainder = Vec::new();
        remainder.extend_from_slice(&encoded[2..10]);
        assert!(decoder.push(&remainder).is_empty());
        let result = decoder.push(&encoded[10..]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].as_ref().unwrap(), &frame);
    }
}
