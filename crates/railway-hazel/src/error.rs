use std::io;
use thiserror::Error;

/// Errors that can occur in the Hazel networking layer.
#[derive(Error, Debug)]
pub enum HazelError {
    /// An I/O error occurred on the underlying socket.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// The connection was refused by the remote end.
    #[error("Connection refused: {0}")]
    ConnectionRefused(String),

    /// The connection timed out.
    #[error("Connection timed out")]
    ConnectionTimeout,

    /// Received invalid or malformed handshake data.
    #[error("Invalid handshake data")]
    InvalidHandshake,

    /// The protocol version is incompatible.
    #[error("Incompatible protocol version: ours={ours}, theirs={theirs}")]
    IncompatibleVersion { ours: u8, theirs: u8 },

    /// Message serialization error (e.g., buffer too large).
    #[error("Message serialization error: {0}")]
    SerializationError(String),

    /// Message deserialization error (e.g., malformed data).
    #[error("Message deserialization error: {0}")]
    DeserializationError(String),

    /// Received an unexpected packet type for the current connection state.
    #[error("Unexpected packet type {packet_type} in state {state:?}")]
    UnexpectedPacket { packet_type: u8, state: String },

    /// Fragment reassembly failed.
    #[error("Fragment error: {0}")]
    FragmentError(String),

    /// The connection is not in a state that allows sending.
    #[error("Cannot send in state {state:?}")]
    CannotSend { state: String },

    /// The send queue is full.
    #[error("Send queue full")]
    SendQueueFull,

    /// Packet exceeds the maximum allowed size.
    #[error("Packet too large: {size} bytes (max: {max})")]
    PacketTooLarge { size: usize, max: usize },

    /// A connection with this endpoint already exists.
    #[error("Connection already exists for {0}")]
    DuplicateConnection(String),

    /// The listener is not running.
    #[error("Listener is not running")]
    ListenerNotRunning,
}
