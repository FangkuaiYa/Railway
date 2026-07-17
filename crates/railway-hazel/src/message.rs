//! Message serialization and deserialization.
//!
//! The Hazel message format:
//! - Each UDP datagram may contain multiple messages.
//! - Each message is: `[length: u16][tag: u8][payload: bytes...]`
//! - Messages can be nested (a message payload can contain sub-messages).
//! - The `MessageWriter` keeps a stack of nesting positions.
//! - The `MessageReader` tracks the current position and nested boundaries.

use bytes::{BufMut, Bytes, BytesMut};
use tracing::trace;

use crate::packed;
use crate::send_option::SendOption;

/// Buffered writer for constructing Hazel protocol messages.
///
/// Supports nested messages via `start_message`/`end_message`.
/// Each message reserves 3 header bytes (2 for length, 1 for tag) that are
/// patched in when `end_message` is called.
pub struct MessageWriter {
    buffer: BytesMut,
    /// Stack of (start_position, tag) for nested messages.
    /// start_position is the position of the length field.
    stack: Vec<(usize, u8)>,
    send_option: SendOption,
}

impl MessageWriter {
    /// Creates a new MessageWriter with the given send option.
    pub fn new(send_option: SendOption) -> Self {
        Self {
            buffer: BytesMut::with_capacity(512),
            stack: Vec::new(),
            send_option,
        }
    }

    /// Returns the send option for this message.
    pub fn send_option(&self) -> SendOption {
        self.send_option
    }

    /// Sets the send option for this message.
    pub fn set_send_option(&mut self, option: SendOption) {
        self.send_option = option;
    }

    /// Starts a new nested message. Writes 3 placeholder bytes for the
    /// length (2 bytes) and tag (1 byte). They are patched when `end_message` is called.
    pub fn start_message(&mut self, tag: u8) {
        let pos = self.buffer.len();
        self.buffer.put_u16_le(0); // placeholder length
        self.buffer.put_u8(tag);
        self.stack.push((pos, tag));
    }

    /// Ends the current nested message, patching the length field.
    ///
    /// # Panics
    /// Panics if there is no matching `start_message` call.
    pub fn end_message(&mut self) {
        let (pos, tag) = self.stack.pop().expect("end_message without start_message");
        let payload_len = self.buffer.len() - pos - 3;
        assert!(
            payload_len <= u16::MAX as usize,
            "message payload too large: {} bytes",
            payload_len
        );
        // Patch the length (little-endian u16)
        self.buffer[pos] = (payload_len & 0xFF) as u8;
        self.buffer[pos + 1] = ((payload_len >> 8) & 0xFF) as u8;
        trace!(tag, payload_len, "end_message");
    }

    // ---- Write methods ----

    pub fn write_byte(&mut self, value: u8) {
        self.buffer.put_u8(value);
    }

    pub fn write_bool(&mut self, value: bool) {
        self.buffer.put_u8(value as u8);
    }

    pub fn write_u16(&mut self, value: u16) {
        self.buffer.put_u16_le(value);
    }

    pub fn write_i16(&mut self, value: i16) {
        self.buffer.put_i16_le(value);
    }

    pub fn write_u32(&mut self, value: u32) {
        self.buffer.put_u32_le(value);
    }

    pub fn write_u64(&mut self, value: u64) {
        self.buffer.put_u64_le(value);
    }

    pub fn write_i32(&mut self, value: i32) {
        self.buffer.put_i32_le(value);
    }

    pub fn write_f32(&mut self, value: f32) {
        self.buffer.put_f32_le(value);
    }

    pub fn write_packed_u32(&mut self, value: u32) {
        packed::write_packed_u32(&mut self.buffer, value);
    }

    pub fn write_packed_i32(&mut self, value: i32) {
        packed::write_packed_i32(&mut self.buffer, value);
    }

    /// Writes a length-prefixed UTF-8 string (length as packed u32).
    pub fn write_string(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
    }

    /// Writes raw bytes with a packed u32 length prefix.
    pub fn write_bytes(&mut self, data: &[u8]) {
        self.write_packed_u32(data.len() as u32);
        self.buffer.put_slice(data);
    }

    /// Writes raw bytes without a length prefix.
    pub fn write_raw(&mut self, data: &[u8]) {
        self.buffer.put_slice(data);
    }

    /// Copies all remaining data from a reader into this writer.
    pub fn copy_from_reader(&mut self, reader: &mut MessageReader) {
        let remaining = reader.remaining();
        let data = reader.buffer[reader.position..reader.position + remaining].to_vec();
        self.buffer.put_slice(&data);
        reader.position += remaining;
    }

    /// Returns the total number of bytes written so far.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns true if no bytes have been written.
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Returns the depth of nested message starts.
    pub fn nesting_depth(&self) -> usize {
        self.stack.len()
    }

    /// Consumes the writer and returns the raw bytes.
    pub fn into_bytes(self) -> Bytes {
        assert!(
            self.stack.is_empty(),
            "MessageWriter destroyed with unclosed messages"
        );
        self.buffer.freeze()
    }

    /// Clears the writer, resetting all state.
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.stack.clear();
    }
}

/// Buffered reader for deserializing Hazel protocol messages.
///
/// Used to read individual messages within a UDP datagram.
/// Each message has its own `MessageReader` with a bounded view into the data.
#[derive(Debug)]
pub struct MessageReader {
    buffer: Bytes,
    position: usize,
    /// The tag byte for this message (set when parsed from the parent).
    pub tag: u8,
}

impl MessageReader {
    /// Creates a new MessageReader from raw bytes.
    pub fn new(buffer: Bytes, tag: u8) -> Self {
        Self {
            buffer,
            position: 0,
            tag,
        }
    }

    /// Returns the number of bytes remaining to read.
    pub fn remaining(&self) -> usize {
        self.buffer.len().saturating_sub(self.position)
    }

    /// Returns the ENTIRE underlying buffer for this message, regardless
    /// of current read position. Useful for caching a sub-message's raw
    /// bytes verbatim (e.g. to replay a spawn message to a late-joining
    /// client) without needing to re-serialize anything.
    pub fn full_buffer(&self) -> Bytes {
        self.buffer.clone()
    }

    /// Reads all remaining bytes as-is, with no length prefix and no
    /// assumed structure. Useful for opaque trailing/unknown data.
    pub fn read_bytes_to_end(&mut self) -> Bytes {
        let slice = self.buffer.slice(self.position..);
        self.position = self.buffer.len();
        slice
    }

    /// Returns the total length of this message's payload.
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Returns true if all bytes have been consumed.
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Returns the current read position.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Returns a slice of the remaining data from the current position.
    pub fn slice_current(&self) -> Bytes {
        self.buffer.slice(self.position..)
    }

    /// Seeks to a specific position (used for re-reading after validation).
    pub fn seek(&mut self, position: usize) {
        assert!(position <= self.buffer.len());
        self.position = position;
    }

    /// Reads the next length-prefixed sub-message, returning a new `MessageReader`
    /// bounded to that message's payload. The tag is read from the sub-message.
    pub fn read_message(&mut self) -> Option<MessageReader> {
        if self.remaining() < 3 {
            return None;
        }

        let len = self.read_u16() as usize;
        let tag = self.read_byte();

        if self.remaining() < len {
            return None;
        }

        let start = self.position;
        self.position += len;

        let sub_buffer = self.buffer.slice(start..start + len);
        Some(MessageReader::new(sub_buffer, tag))
    }

    /// Removes a sub-message from the parent buffer. Used when the parent
    /// needs to skip a message that was already read (e.g., after validation).
    pub fn remove_message(&mut self, _child: &MessageReader) {
        // In the C# implementation, this removes the child message bytes from the
        // parent so they aren't broadcast. For our Rust implementation, the parent
        // reader's position already advanced past the child, so this is a no-op.
        // The broadcast logic handles which messages to relay separately.
    }

    // ---- Read methods ----

    pub fn read_byte(&mut self) -> u8 {
        if self.position >= self.buffer.len() {
            tracing::warn!(position = self.position, buffer_len = self.buffer.len(), "read_byte: past end of buffer, returning 0");
            return 0;
        }
        let b = self.buffer[self.position];
        self.position += 1;
        b
    }

    pub fn read_bool(&mut self) -> bool {
        self.read_byte() != 0
    }

    pub fn read_u16(&mut self) -> u16 {
        let mut bytes = [0u8; 2];
        let end = self.position + 2;
        if end > self.buffer.len() {
            tracing::warn!(position = self.position, buffer_len = self.buffer.len(), "read_u16: past end of buffer, returning 0");
            self.position = self.buffer.len();
            return 0;
        }
        bytes.copy_from_slice(&self.buffer[self.position..end]);
        self.position = end;
        u16::from_le_bytes(bytes)
    }

    pub fn read_u32(&mut self) -> u32 {
        let mut bytes = [0u8; 4];
        let end = self.position + 4;
        if end > self.buffer.len() {
            tracing::warn!(position = self.position, buffer_len = self.buffer.len(), "read_u32: past end of buffer, returning 0");
            self.position = self.buffer.len();
            return 0;
        }
        bytes.copy_from_slice(&self.buffer[self.position..end]);
        self.position = end;
        u32::from_le_bytes(bytes)
    }

    pub fn read_u64(&mut self) -> u64 {
        let mut bytes = [0u8; 8];
        let end = self.position + 8;
        if end > self.buffer.len() {
            tracing::warn!(position = self.position, buffer_len = self.buffer.len(), "read_u64: past end of buffer, returning 0");
            self.position = self.buffer.len();
            return 0;
        }
        bytes.copy_from_slice(&self.buffer[self.position..end]);
        self.position = end;
        u64::from_le_bytes(bytes)
    }

    pub fn read_i32(&mut self) -> i32 {
        self.read_u32() as i32
    }

    pub fn read_f32(&mut self) -> f32 {
        let bits = self.read_u32();
        f32::from_bits(bits)
    }

    pub fn read_packed_u32(&mut self) -> u32 {
        let mut slice = &self.buffer[self.position..];
        let value = packed::read_packed_u32(&mut slice).expect("failed to read packed u32");
        let consumed = self.buffer.len() - self.position - slice.len();
        self.position += consumed;
        value
    }

    pub fn read_packed_i32(&mut self) -> i32 {
        let mut slice = &self.buffer[self.position..];
        let value = packed::read_packed_i32(&mut slice).expect("failed to read packed i32");
        let consumed = self.buffer.len() - self.position - slice.len();
        self.position += consumed;
        value
    }

    /// Reads a length-prefixed UTF-8 string.
    pub fn read_string(&mut self) -> String {
        let len = self.read_packed_u32() as usize;
        let end = self.position + len;
        if end > self.buffer.len() {
            tracing::warn!(
                requested_len = len,
                position = self.position,
                buffer_len = self.buffer.len(),
                "read_string: length prefix overruns buffer — likely a protocol desync upstream; \
                 truncating instead of panicking"
            );
            let bytes = &self.buffer[self.position..];
            self.position = self.buffer.len();
            return String::from_utf8_lossy(bytes).into_owned();
        }
        let bytes = &self.buffer[self.position..end];
        self.position = end;
        String::from_utf8_lossy(bytes).into_owned()
    }

    /// Reads length-prefixed raw bytes.
    pub fn read_bytes(&mut self) -> Bytes {
        let len = self.read_packed_u32() as usize;
        let end = self.position + len;
        if end > self.buffer.len() {
            tracing::warn!(
                requested_len = len,
                position = self.position,
                buffer_len = self.buffer.len(),
                "read_bytes: length prefix overruns buffer — likely a protocol desync upstream; \
                 truncating instead of panicking"
            );
            let slice = self.buffer.slice(self.position..);
            self.position = self.buffer.len();
            return slice;
        }
        let slice = self.buffer.slice(self.position..end);
        self.position = end;
        slice
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nested_messages() {
        let mut writer = MessageWriter::new(SendOption::Reliable);

        writer.start_message(0x05); // outer message tag
        writer.write_byte(42);

        writer.start_message(0x01); // inner message tag
        writer.write_packed_u32(12345);
        writer.end_message(); // inner

        writer.end_message(); // outer

        let bytes = writer.into_bytes();

        // Parse back
        let mut reader = MessageReader::new(bytes, 0xFF);

        // The outer message should be readable as a sub-message
        let outer = reader.read_message().unwrap();
        assert_eq!(outer.tag, 0x05);
        // outer payload: byte(42) + inner_message
        let mut outer_reader = MessageReader::new(
            outer.buffer.slice(outer.position..),
            outer.tag,
        );

        assert_eq!(outer_reader.read_byte(), 42);

        let inner = outer_reader.read_message().unwrap();
        assert_eq!(inner.tag, 0x01);
        assert_eq!(inner.read_packed_u32(), 12345);
    }

    #[test]
    fn test_string_roundtrip() {
        let mut writer = MessageWriter::new(SendOption::Reliable);
        writer.start_message(0x01);
        writer.write_string("Hello, World!");
        writer.end_message();

        let bytes = writer.into_bytes();
        let mut reader = MessageReader::new(bytes, 0xFF);
        let sub = reader.read_message().unwrap();
        assert_eq!(sub.read_string(), "Hello, World!");
    }

    #[test]
    fn test_multiple_messages_in_packet() {
        let mut writer = MessageWriter::new(SendOption::Reliable);
        writer.start_message(0x01);
        writer.write_byte(1);
        writer.end_message();
        writer.start_message(0x02);
        writer.write_byte(2);
        writer.end_message();

        let bytes = writer.into_bytes();
        let mut reader = MessageReader::new(bytes, 0xFF);

        let msg1 = reader.read_message().unwrap();
        assert_eq!(msg1.tag, 0x01);
        assert_eq!(msg1.buffer[0], 1);

        let msg2 = reader.read_message().unwrap();
        assert_eq!(msg2.tag, 0x02);
        assert_eq!(msg2.buffer[0], 2);
    }

    #[test]
    fn test_packed_int_in_message() {
        let mut writer = MessageWriter::new(SendOption::Reliable);
        writer.start_message(0x01);
        writer.write_packed_u32(300);
        writer.write_packed_i32(-42);
        writer.end_message();

        let bytes = writer.into_bytes();
        let mut reader = MessageReader::new(bytes, 0xFF);
        let sub = reader.read_message().unwrap();
        assert_eq!(sub.read_packed_u32(), 300);
        assert_eq!(sub.read_packed_i32(), -42);
    }
}
