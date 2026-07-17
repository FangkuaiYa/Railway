//! # railway-hazel
//!
//! A standalone implementation of the Hazel UDP networking protocol used by Among Us.
//!
//! This crate provides a reliability layer over UDP with:
//! - Connection state management (hello handshake, connected, disconnect)
//! - Reliable and unreliable message delivery
//! - Automatic retransmission with exponential backoff
//! - Message fragmentation and reassembly
//! - Ping measurement
//! - Keep-alive heartbeats
//!
//! The crate has no knowledge of Among Us game logic — it is a pure transport layer.

pub mod connection;
pub mod constants;
pub mod error;
pub mod fragmentation;
pub mod listener;
pub mod message;
pub mod packed;
pub mod reliability;
pub mod send_option;
pub mod state;

#[cfg(test)]
mod tests;

pub use connection::Connection;
pub use error::HazelError;
pub use listener::UdpConnectionListener;
pub use message::{MessageReader, MessageWriter};
pub use packed::{read_packed_i32, read_packed_u32, write_packed_i32, write_packed_u32};
pub use send_option::SendOption;
pub use state::ConnectionState;

// Re-exports for use by dependents

/// Result type alias for Hazel operations.
pub type HazelResult<T> = Result<T, HazelError>;

/// The protocol version used by this Hazel implementation.
pub const HAZEL_VERSION: u8 = 0;

/// Maximum size of a UDP datagram payload (without IP/UDP headers).
pub const MAX_UDP_PAYLOAD_SIZE: usize = 1200;

/// Number of recent sequence numbers tracked in the ACK bitmap.
pub const ACK_BITFIELD_SIZE: usize = 8; // 8 bytes = 64 bits for ACK bitfield
