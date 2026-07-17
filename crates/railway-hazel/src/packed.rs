//! Packed integer encoding/decoding.
//!
//! Among Us uses a variable-length integer encoding similar to LEB128
//! but with a slightly different continuation scheme.
//!
//! Each byte uses the lower 7 bits for data and the MSB as a continuation
//! flag (1 = more bytes follow, 0 = last byte).
//!
//! For "signed" packed ints, real Hazel does NOT use zigzag encoding —
//! `WritePacked(int)`/`ReadPackedInt32()` in the C# implementation just
//! reinterpret the i32's bit pattern as a u32 and run it through the same
//! unsigned VLQ routine. See `write_packed_i32`/`read_packed_i32` below.

use bytes::{Buf, BufMut};

/// Write an unsigned 32-bit integer in packed variable-length format.
///
/// Returns the number of bytes written.
pub fn write_packed_u32(buf: &mut impl BufMut, value: u32) -> usize {
    let mut v = value;
    let mut written = 0;

    loop {
        let mut byte = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        buf.put_u8(byte);
        written += 1;
        if v == 0 {
            break;
        }
    }

    written
}

/// Write a signed 32-bit integer in packed variable-length format.
///
/// IMPORTANT: real Hazel/Among Us does NOT zigzag-encode signed packed
/// ints. `MessageWriter.WritePacked(int value)` in the C# implementation is
/// simply `this.WritePacked((uint)value)` — a raw bit-pattern cast, with no
/// zigzag transform at all. Using zigzag here (as a previous version of
/// this function did) produces a completely different byte stream for any
/// value that isn't tiny, which for something like `GameVersion` (always a
/// large positive number) corrupts the decoded value on the server side.
/// That in turn makes the handshake's `client_version >= V1/V2/V3/V4`
/// branches take the wrong path, causing fields to be wrongly skipped or
/// expected, which desyncs every read after it — this is what produced the
/// "range end index N out of range" panics seen in testing.
///
/// Returns the number of bytes written.
pub fn write_packed_i32(buf: &mut impl BufMut, value: i32) -> usize {
    write_packed_u32(buf, value as u32)
}

/// Read an unsigned 32-bit integer in packed variable-length format.
///
/// Returns an error if the buffer runs out before the value is complete.
pub fn read_packed_u32(buf: &mut impl Buf) -> Result<u32, PackedReadError> {
    let mut value: u32 = 0;
    let mut shift: u32 = 0;

    loop {
        if !buf.has_remaining() {
            return Err(PackedReadError::UnexpectedEof);
        }
        let byte = buf.get_u8();
        value |= ((byte & 0x7F) as u32) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 32 {
            return Err(PackedReadError::Overflow);
        }
    }

    Ok(value)
}

/// Read a signed 32-bit integer in packed variable-length format.
///
/// See `write_packed_i32` for why this must be a plain bit-cast, not a
/// zigzag decode.
pub fn read_packed_i32(buf: &mut impl Buf) -> Result<i32, PackedReadError> {
    Ok(read_packed_u32(buf)? as i32)
}

/// Errors that can occur while reading packed integers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackedReadError {
    /// The buffer ran out before the integer was fully read.
    UnexpectedEof,
    /// The encoded value would overflow 32 bits.
    Overflow,
}

impl std::fmt::Display for PackedReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of buffer while reading packed int"),
            Self::Overflow => write!(f, "packed integer overflow (would exceed 32 bits)"),
        }
    }
}

impl std::error::Error for PackedReadError {}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn test_roundtrip_u32() {
        let test_values = [
            0u32,
            1,
            127,
            128,
            255,
            256,
            16383,
            16384,
            2097151,
            2097152,
            268435455,
            268435456,
            u32::MAX,
        ];

        for &expected in &test_values {
            let mut buf = BytesMut::new();
            write_packed_u32(&mut buf, expected);
            let mut cursor = buf.freeze();
            let result = read_packed_u32(&mut cursor).unwrap();
            assert_eq!(result, expected, "roundtrip failed for {}", expected);
            assert!(
                !cursor.has_remaining(),
                "extra bytes remaining for {}",
                expected
            );
        }
    }

    #[test]
    fn test_roundtrip_i32() {
        let test_values = [
            0i32,
            1,
            -1,
            127,
            -127,
            128,
            -128,
            16383,
            -16383,
            16384,
            -16384,
            i32::MAX,
            i32::MIN,
        ];

        for &expected in &test_values {
            let mut buf = BytesMut::new();
            write_packed_i32(&mut buf, expected);
            let mut cursor = buf.freeze();
            let result = read_packed_i32(&mut cursor).unwrap();
            assert_eq!(result, expected, "roundtrip failed for {}", expected);
            assert!(
                !cursor.has_remaining(),
                "extra bytes remaining for {}",
                expected
            );
        }
    }

    #[test]
    fn test_packed_small_values_one_byte() {
        for v in 0..128u32 {
            let mut buf = BytesMut::new();
            let written = write_packed_u32(&mut buf, v);
            assert_eq!(written, 1, "value {} should use 1 byte", v);
            let mut cursor = buf.freeze();
            assert_eq!(read_packed_u32(&mut cursor).unwrap(), v);
        }
    }

    #[test]
    fn test_packed_eof() {
        // Empty buffer
        let mut buf = BytesMut::new();
        buf.put_u8(0x80); // Continuation bit set but no more data
        let mut cursor = buf.freeze();
        assert!(read_packed_u32(&mut cursor).is_err());
    }
}
