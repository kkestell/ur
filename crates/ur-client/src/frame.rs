use std::io;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use serde::Serialize;
use tokio_util::codec::{Decoder, Encoder};

use crate::protocol::TerminalId;

const TAG_JSON: u8 = 0x01;
const TAG_PTY: u8 = 0x02;
const HEADER_LEN: usize = 5;
const TERMINAL_ID_LEN: usize = 4;

/// One message on the daemon socket: a one-byte tag, a big-endian `u32`
/// payload length, and the payload. The `u32` length is the only bound on a
/// payload's size: the decoder accepts any length, because both ends of the
/// socket are ur on the local machine.
#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    Json(Bytes),
    Pty { id: TerminalId, bytes: Bytes },
}

impl Frame {
    pub fn json<T: Serialize>(message: &T) -> Frame {
        let json = serde_json::to_vec(message).expect("wire protocol messages serialize");
        Frame::Json(json.into())
    }
}

pub struct FrameCodec;

impl Decoder for FrameCodec {
    type Item = Frame;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> io::Result<Option<Frame>> {
        if src.len() < HEADER_LEN {
            return Ok(None);
        }
        let tag = src[0];
        let len = u32::from_be_bytes([src[1], src[2], src[3], src[4]]) as usize;
        if tag != TAG_JSON && tag != TAG_PTY {
            return Err(invalid(format!("unknown frame tag {tag:#04x}")));
        }
        if tag == TAG_PTY && len < TERMINAL_ID_LEN {
            return Err(invalid("PTY frame is shorter than a terminal ID".into()));
        }
        if src.len() < HEADER_LEN + len {
            src.reserve(HEADER_LEN + len - src.len());
            return Ok(None);
        }
        src.advance(HEADER_LEN);
        let mut payload = src.split_to(len).freeze();
        Ok(Some(match tag {
            TAG_JSON => Frame::Json(payload),
            _ => {
                let id = payload.get_u32();
                Frame::Pty { id, bytes: payload }
            }
        }))
    }
}

impl Encoder<Frame> for FrameCodec {
    type Error = io::Error;

    fn encode(&mut self, frame: Frame, dst: &mut BytesMut) -> io::Result<()> {
        match frame {
            Frame::Json(json) => {
                dst.reserve(HEADER_LEN + json.len());
                dst.put_u8(TAG_JSON);
                dst.put_u32(json.len() as u32);
                dst.put_slice(&json);
            }
            Frame::Pty { id, bytes } => {
                dst.reserve(HEADER_LEN + TERMINAL_ID_LEN + bytes.len());
                dst.put_u8(TAG_PTY);
                dst.put_u32((TERMINAL_ID_LEN + bytes.len()) as u32);
                dst.put_u32(id);
                dst.put_slice(&bytes);
            }
        }
        Ok(())
    }
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let frames = [
            Frame::Json(Bytes::from_static(br#"{"id":1}"#)),
            Frame::Pty {
                id: 7,
                bytes: Bytes::from_static(b"\x1b[31mred\r\n"),
            },
            Frame::Json(Bytes::from(vec![b'x'; 20 * 1024 * 1024])),
        ];
        for frame in frames {
            let mut buffer = BytesMut::new();
            FrameCodec.encode(frame.clone(), &mut buffer).unwrap();
            let decoded = FrameCodec.decode(&mut buffer).unwrap();
            assert_eq!(decoded, Some(frame));
            assert!(buffer.is_empty());
        }
    }

    #[test]
    fn decoder_rejects_malformed_frames() {
        let cases: [(&str, Vec<u8>); 2] = [
            ("unknown tag", vec![0x03, 0, 0, 0, 0]),
            (
                "PTY payload shorter than a terminal ID",
                vec![TAG_PTY, 0, 0, 0, 3, 0, 0, 1],
            ),
        ];
        for (name, bytes) in cases {
            let mut buffer = BytesMut::from(&bytes[..]);
            let error = FrameCodec.decode(&mut buffer).expect_err(name);
            assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{name}");
        }
    }
}
