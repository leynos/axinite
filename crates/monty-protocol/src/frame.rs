//! Bounded length-prefixed framing.

use std::io::{self, Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::json::validate_json_text;

/// Bytes in the big-endian length prefix.
pub const LENGTH_PREFIX_BYTES: usize = 4;

/// Largest JSON payload either peer accepts on the wire.
pub const MAX_FRAME_BYTES: usize = 256 * 1024;

/// A framing, encoding or decoding failure.
#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    /// A frame announced a zero-length payload.
    #[error("frame length is zero")]
    ZeroLength,
    /// A frame announced more bytes than the ceiling.
    #[error("frame length {length} exceeds the {max}-byte ceiling")]
    Oversized {
        /// The announced length.
        length: usize,
        /// The configured ceiling.
        max: usize,
    },
    /// The peer closed or truncated the stream.
    #[error("truncated frame: expected {expected} bytes, received {received}")]
    Truncated {
        /// Bytes required.
        expected: usize,
        /// Bytes actually read.
        received: usize,
    },
    /// The payload was not valid UTF-8.
    #[error("frame payload is not valid UTF-8")]
    InvalidUtf8,
    /// The payload exceeded a structural limit or carried an unsupported
    /// number.
    #[error(transparent)]
    Limit(#[from] crate::json::JsonLimitError),
    /// The payload was not valid JSON, had trailing data, or did not match the
    /// expected variant.
    #[error("invalid JSON frame: {0}")]
    InvalidJson(#[from] serde_json::Error),
    /// Serializing a message produced more than the frame ceiling.
    #[error("encoded frame length {0} exceeds the frame ceiling")]
    OutputTooLarge(usize),
    /// The underlying pipe failed.
    #[error("frame I/O failed: {0}")]
    Io(#[from] io::Error),
}

/// Encodes one message as a complete frame.
pub fn encode_frame<T: Serialize>(message: &T) -> Result<Vec<u8>, FrameError> {
    let payload = serde_json::to_vec(message)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(FrameError::OutputTooLarge(payload.len()));
    }
    let length =
        u32::try_from(payload.len()).map_err(|_| FrameError::OutputTooLarge(payload.len()))?;
    let mut framed = Vec::with_capacity(LENGTH_PREFIX_BYTES + payload.len());
    framed.extend_from_slice(&length.to_be_bytes());
    framed.extend_from_slice(&payload);
    Ok(framed)
}

/// Writes one message as a complete frame and flushes the writer.
pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, message: &T) -> Result<(), FrameError> {
    let framed = encode_frame(message)?;
    writer.write_all(&framed)?;
    writer.flush()?;
    Ok(())
}

/// Reads one complete frame payload, validating its length and JSON limits.
///
/// The length ceiling is enforced before the payload buffer is allocated.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<Vec<u8>, FrameError> {
    let mut prefix = [0_u8; LENGTH_PREFIX_BYTES];
    read_exact_or_truncated(reader, &mut prefix)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 {
        return Err(FrameError::ZeroLength);
    }
    if length > MAX_FRAME_BYTES {
        return Err(FrameError::Oversized {
            length,
            max: MAX_FRAME_BYTES,
        });
    }
    let mut payload = vec![0_u8; length];
    read_exact_or_truncated(reader, &mut payload)?;
    let text = std::str::from_utf8(&payload).map_err(|_| FrameError::InvalidUtf8)?;
    validate_json_text(text)?;
    Ok(payload)
}

/// Decodes a frame payload into a typed message.
///
/// `serde_json` rejects trailing non-whitespace data, and the message types
/// reject unknown fields.
pub fn decode_frame<T: DeserializeOwned>(payload: &[u8]) -> Result<T, FrameError> {
    Ok(serde_json::from_slice(payload)?)
}

fn read_exact_or_truncated<R: Read>(reader: &mut R, buffer: &mut [u8]) -> Result<(), FrameError> {
    match reader.read_exact(buffer) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Err(FrameError::Truncated {
            expected: buffer.len(),
            received: 0,
        }),
        Err(error) => Err(FrameError::Io(error)),
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the length-framed codec.

    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn round_trip_frame() {
        let mut buffer = Vec::new();
        write_frame(&mut buffer, &serde_json::json!({"kind": "hello"})).expect("write");
        assert_eq!(&buffer[..4], &[0, 0, 0, 16]);
        let payload = read_frame(&mut buffer.as_slice()).expect("read");
        assert_eq!(
            decode_frame::<serde_json::Value>(&payload).expect("decode"),
            serde_json::json!({"kind": "hello"})
        );
    }

    #[test]
    fn rejects_zero_length() {
        let error = read_frame(&mut [0_u8; 4].as_slice()).expect_err("zero");
        assert!(matches!(error, FrameError::ZeroLength));
    }

    #[test]
    fn rejects_oversized_before_allocation() {
        let mut prefix = Vec::from(u32::try_from(MAX_FRAME_BYTES + 1).unwrap().to_be_bytes());
        prefix.extend_from_slice(b"{}");
        let error = read_frame(&mut prefix.as_slice()).expect_err("oversized");
        assert!(matches!(error, FrameError::Oversized { .. }));
    }

    #[test]
    fn rejects_truncated_payload() {
        let mut bytes = Vec::from(10_u32.to_be_bytes());
        bytes.extend_from_slice(b"{}");
        let error = read_frame(&mut bytes.as_slice()).expect_err("truncated");
        assert!(matches!(error, FrameError::Truncated { expected: 10, .. }));
    }

    #[test]
    fn rejects_trailing_data() {
        let payload = br#"{"kind":"hello"} trailing"#;
        let error = decode_frame::<serde_json::Value>(payload).expect_err("trailing");
        assert!(matches!(error, FrameError::InvalidJson(_)));
    }

    #[test]
    fn rejects_invalid_utf8() {
        let mut bytes = Vec::from(2_u32.to_be_bytes());
        bytes.extend_from_slice(&[0xff, 0xfe]);
        let error = read_frame(&mut bytes.as_slice()).expect_err("utf8");
        assert!(matches!(error, FrameError::InvalidUtf8));
    }
}
