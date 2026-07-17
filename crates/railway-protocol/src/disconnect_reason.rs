//! Disconnect reasons sent to clients when they are removed from a game.

/// Reason a player was disconnected from a game.
/// Values must match the official Among Us protocol exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DisconnectReason {
    /// Player exited the game normally.
    ExitGame = 0,
    /// The game is full.
    GameFull = 1,
    /// The game has already started.
    GameStarted = 2,
    /// The requested game was not found.
    GameNotFound = 3,
    /// Incorrect client version.
    IncorrectVersion = 5,
    /// Player is banned from the game.
    Banned = 6,
    /// Player was kicked from the game.
    Kicked = 7,
    /// Custom disconnect message.
    Custom = 8,
    /// Invalid player name.
    InvalidName = 9,
    /// Player was caught hacking.
    Hacking = 10,
    /// Player is not authorized.
    NotAuthorized = 11,
    /// Server connection limit reached.
    ConnectionLimit = 12,
    /// The game was destroyed.
    Destroy = 16,
    /// Generic error.
    Error = 17,
    /// Incorrect game specified.
    IncorrectGame = 18,
    /// Server requested the disconnect.
    ServerRequest = 19,
    /// Server is full.
    ServerFull = 20,
    /// Client version doesn't match server.
    MismatchedVersion = 21,

    // ── Internal / platform reasons ──
    /// Internal: player missing.
    InternalPlayerMissing = 100,
    /// Internal: nonce failure.
    InternalNonceFailure = 101,
    /// Internal: connection token issue.
    InternalConnectionToken = 102,
    /// Player is platform-locked.
    PlatformLock = 103,
    /// Lobby inactivity timeout.
    LobbyInactivity = 104,
    /// Matchmaker inactivity timeout.
    MatchmakerInactivity = 105,
    /// Invalid game options.
    InvalidGameOptions = 106,
    /// No servers available.
    NoServersAvailable = 107,
    /// Quickmatch is disabled.
    QuickmatchDisabled = 108,
    /// Too many games in progress.
    TooManyGames = 109,
    /// Quickchat is locked.
    QuickchatLock = 110,
    /// Matchmaker is full.
    MatchmakerFull = 111,
    /// Player has sanctions.
    Sanctions = 112,
    /// Internal server error.
    ServerError = 113,
    /// Self platform lock.
    SelfPlatformLock = 114,
    /// Duplicate connection detected.
    DuplicateConnectionDetected = 115,
    /// Too many requests from client.
    TooManyRequests = 116,

    // ── Focus / leaving reasons ──
    /// Client lost focus in background.
    FocusLostBackground = 207,
    /// Player intentionally left.
    IntentionalLeaving = 208,
    /// Client lost focus.
    FocusLost = 209,
    /// New connection from same account.
    NewConnection = 210,
    /// Platform parental controls block.
    PlatformParentalControlsBlock = 211,
    /// Platform user block.
    PlatformUserBlock = 212,
    /// Platform failed to get user block info.
    PlatformFailedToGetUserBlock = 213,
    /// Server not found.
    ServerNotFound = 214,
    /// Client connection timed out.
    ClientTimeout = 215,
    /// Auth nonce failure.
    ErrorAuthNonceFailure = 216,

    /// Unknown reason (not in the official protocol).
    Unknown = 255,
}

impl DisconnectReason {
    /// Try to convert a byte value to a DisconnectReason.
    pub fn from_byte(byte: u8) -> Self {
        match byte {
            0 => Self::ExitGame,
            1 => Self::GameFull,
            2 => Self::GameStarted,
            3 => Self::GameNotFound,
            5 => Self::IncorrectVersion,
            6 => Self::Banned,
            7 => Self::Kicked,
            8 => Self::Custom,
            9 => Self::InvalidName,
            10 => Self::Hacking,
            11 => Self::NotAuthorized,
            12 => Self::ConnectionLimit,
            16 => Self::Destroy,
            17 => Self::Error,
            18 => Self::IncorrectGame,
            19 => Self::ServerRequest,
            20 => Self::ServerFull,
            21 => Self::MismatchedVersion,
            100 => Self::InternalPlayerMissing,
            101 => Self::InternalNonceFailure,
            102 => Self::InternalConnectionToken,
            103 => Self::PlatformLock,
            104 => Self::LobbyInactivity,
            105 => Self::MatchmakerInactivity,
            106 => Self::InvalidGameOptions,
            107 => Self::NoServersAvailable,
            108 => Self::QuickmatchDisabled,
            109 => Self::TooManyGames,
            110 => Self::QuickchatLock,
            111 => Self::MatchmakerFull,
            112 => Self::Sanctions,
            113 => Self::ServerError,
            114 => Self::SelfPlatformLock,
            115 => Self::DuplicateConnectionDetected,
            116 => Self::TooManyRequests,
            207 => Self::FocusLostBackground,
            208 => Self::IntentionalLeaving,
            209 => Self::FocusLost,
            210 => Self::NewConnection,
            211 => Self::PlatformParentalControlsBlock,
            212 => Self::PlatformUserBlock,
            213 => Self::PlatformFailedToGetUserBlock,
            214 => Self::ServerNotFound,
            215 => Self::ClientTimeout,
            216 => Self::ErrorAuthNonceFailure,
            _ => Self::Unknown,
        }
    }

    /// Returns a human-readable description.
    pub fn description(self) -> &'static str {
        match self {
            Self::ExitGame => "Exited game",
            Self::GameFull => "Game is full",
            Self::GameStarted => "Game already started",
            Self::GameNotFound => "Game not found",
            Self::IncorrectVersion => "Incorrect version",
            Self::Banned => "Banned",
            Self::Kicked => "Kicked",
            Self::Custom => "Custom",
            Self::InvalidName => "Invalid name",
            Self::Hacking => "Hacking",
            Self::NotAuthorized => "Not authorized",
            Self::ConnectionLimit => "Connection limit reached",
            Self::Destroy => "Game destroyed",
            Self::Error => "Error",
            Self::IncorrectGame => "Incorrect game",
            Self::ServerRequest => "Server request",
            Self::ServerFull => "Server is full",
            Self::MismatchedVersion => "Client version mismatch",
            Self::InternalPlayerMissing => "Internal: player missing",
            Self::InternalNonceFailure => "Internal: nonce failure",
            Self::InternalConnectionToken => "Internal: connection token",
            Self::PlatformLock => "Platform lock",
            Self::LobbyInactivity => "Lobby inactivity",
            Self::MatchmakerInactivity => "Matchmaker inactivity",
            Self::InvalidGameOptions => "Invalid game options",
            Self::NoServersAvailable => "No servers available",
            Self::QuickmatchDisabled => "Quickmatch disabled",
            Self::TooManyGames => "Too many games",
            Self::QuickchatLock => "Quickchat lock",
            Self::MatchmakerFull => "Matchmaker full",
            Self::Sanctions => "Sanctions",
            Self::ServerError => "Server error",
            Self::SelfPlatformLock => "Self platform lock",
            Self::DuplicateConnectionDetected => "Duplicate connection",
            Self::TooManyRequests => "Too many requests",
            Self::FocusLostBackground => "Focus lost (background)",
            Self::IntentionalLeaving => "Intentional leaving",
            Self::FocusLost => "Focus lost",
            Self::NewConnection => "New connection",
            Self::PlatformParentalControlsBlock => "Parental controls block",
            Self::PlatformUserBlock => "User block",
            Self::PlatformFailedToGetUserBlock => "Failed to get user block",
            Self::ServerNotFound => "Server not found",
            Self::ClientTimeout => "Client timeout",
            Self::ErrorAuthNonceFailure => "Auth nonce failure",
            Self::Unknown => "Unknown",
        }
    }
}
