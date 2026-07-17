use std::time::Duration;

/// Magic byte sent in initial hello packet from client.
pub const HELLO_MAGIC: u8 = 0x46; // 'H' - not actually used, determined by send option

/// Send option byte values in the packet header.
pub mod send_option_byte {
    pub const UNRELIABLE: u8 = 0;
    pub const RELIABLE: u8 = 1;
    pub const HELLO: u8 = 8;
    pub const DISCONNECT: u8 = 9;
    pub const ACKNOWLEDGMENT: u8 = 10;
    pub const FRAGMENT: u8 = 11;
    pub const PING: u8 = 12;
}

/// Default timeout for the initial hello handshake.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// How often to send keep-alive pings when idle.
/// Matches C# Hazel `KeepAliveInterval = 1500ms`.
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_millis(1500);

/// Time without receiving any data before considering the connection dead.
/// Matches C# Hazel `MissingPingsUntilDisconnect = 6` × `KeepAliveInterval = 1500ms`.
pub const CONNECTION_TIMEOUT: Duration = Duration::from_millis(9000);

/// Maximum time before a reliable packet is considered lost (matching C# DisconnectTimeout).
pub const DISCONNECT_TIMEOUT_MS: u64 = 5000;

/// Maximum number of times to retry sending a reliable message.
pub const MAX_RESEND_ATTEMPTS: u32 = 10;

/// Maximum number of reliable messages that can be in-flight (unacknowledged).
pub const MAX_IN_FLIGHT: usize = 256;

/// Size of the resend queue.
pub const RESEND_QUEUE_SIZE: usize = 1024;

/// The byte size threshold above which reliable messages are automatically fragmented.
pub const FRAGMENT_THRESHOLD: usize = 1000;

/// Maximum fragment payload size (total datagram size minus header overhead).
pub const MAX_FRAGMENT_SIZE: usize = 1000;

/// How many fragments a single message can be split into.
pub const MAX_FRAGMENTS: u8 = 64;
