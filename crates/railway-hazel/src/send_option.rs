/// Specifies how a message should be delivered.
///
/// Corresponds to the byte value in the packet header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SendOption {
    /// No reliability guarantees. Packet may be lost, duplicated, or reordered.
    /// Lowest overhead.
    Unreliable = 0,

    /// Guaranteed delivery with automatic retransmission.
    /// Messages are delivered in order via sequence numbers.
    Reliable = 1,

    /// Used internally for the hello handshake.
    Hello = 8,

    /// Used internally for graceful disconnection.
    Disconnect = 9,

    /// Used internally for ACK packets.
    Acknowledgment = 10,

    /// Used internally for fragmented message assembly.
    Fragment = 11,

    /// Used internally for ping measurement.
    Ping = 12,
}

impl SendOption {
    /// Try to convert a byte value to a SendOption.
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Unreliable),
            1 => Some(Self::Reliable),
            8 => Some(Self::Hello),
            9 => Some(Self::Disconnect),
            10 => Some(Self::Acknowledgment),
            11 => Some(Self::Fragment),
            12 => Some(Self::Ping),
            _ => None,
        }
    }

    /// Convert to the wire byte value.
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Returns true for data-carrying send options.
    pub fn is_data(self) -> bool {
        matches!(self, Self::Unreliable | Self::Reliable)
    }
}
