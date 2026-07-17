use std::fmt;

/// Represents the state of a Hazel connection.
///
/// The state machine transitions as follows:
/// ```text
/// NotConnected → HelloSent → Connected → Disconnecting → NotConnected
///              → HelloReceived → Connected → ...
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// No connection exists yet.
    NotConnected,

    /// Client sent a hello, waiting for server to accept.
    HelloReceived,

    /// Server sent accept, waiting for client acknowledgment.
    HelloAccepted,

    /// Fully connected, can send and receive game data.
    Connected,

    /// Disconnect has been initiated, waiting for confirmation.
    Disconnecting,

    /// The connection is closed/cleaned up.
    Disconnected,
}

impl ConnectionState {
    /// Returns true if the connection is in a state where data can be sent.
    pub fn can_send(self) -> bool {
        matches!(self, Self::Connected)
    }

    /// Returns true if the connection is in a state where data can be received.
    pub fn can_receive(self) -> bool {
        matches!(
            self,
            Self::HelloReceived | Self::HelloAccepted | Self::Connected
        )
    }

    /// Returns true if the connection is alive (not disconnected).
    pub fn is_alive(self) -> bool {
        !matches!(self, Self::Disconnected | Self::NotConnected)
    }
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConnected => write!(f, "NotConnected"),
            Self::HelloReceived => write!(f, "HelloReceived"),
            Self::HelloAccepted => write!(f, "HelloAccepted"),
            Self::Connected => write!(f, "Connected"),
            Self::Disconnecting => write!(f, "Disconnecting"),
            Self::Disconnected => write!(f, "Disconnected"),
        }
    }
}
