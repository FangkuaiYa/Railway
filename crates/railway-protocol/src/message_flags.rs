//! Top-level message flags used in the Hazel message protocol.
//!
//! Each Hazel message has a 1-byte tag identifying its type.
//! These are the Among Us application-level message types.

/// Message flag identifying the type of a top-level Hazel message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageFlags {
    /// Client requests to host a new game. Contains game options.
    HostGame = 0,
    /// Client requests to join an existing game. Contains game code.
    JoinGame = 1,
    /// Host starts the game.
    StartGame = 2,
    /// Remove/destroy a game.
    RemoveGame = 3,
    /// Remove a player from a game.
    RemovePlayer = 4,
    /// Game data payload (broadcast to all).
    GameData = 5,
    /// Game data payload (sent to a specific player).
    GameDataTo = 6,
    /// Response: player successfully joined a game.
    JoinedGame = 7,
    /// Game ended notification.
    EndGame = 8,
    /// Alter game settings (e.g., privacy).
    AlterGame = 10,
    /// Kick a player from the game.
    KickPlayer = 11,
    /// Client is waiting for the host.
    WaitForHost = 12,
    /// Server redirects client to another server.
    Redirect = 13,
    /// Client requests to reselect server.
    ReselectServer = 14,
    /// Client requests the public game list (V2).
    GetGameListV2 = 16,
    /// Report another player.
    ReportPlayer = 17,
    /// Quick match request.
    QuickMatch = 18,
    /// Quick match host response.
    QuickMatchHost = 19,
    /// Set the game session info.
    SetGameSession = 20,
    /// Set the active pod type (platform).
    SetActivePodType = 21,
    /// Query platform-specific IDs for players in a game.
    QueryPlatformIds = 22,
    /// Query lobby info.
    QueryLobbyInfo = 23,
    /// End game with host migration.
    EndGameHostMigration = 24,
    /// Packed game data (multiple GameDataTo messages bundled together).
    PackedGameDataTo = 26,
}

impl MessageFlags {
    /// Try to convert a byte to a MessageFlags.
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::HostGame),
            1 => Some(Self::JoinGame),
            2 => Some(Self::StartGame),
            3 => Some(Self::RemoveGame),
            4 => Some(Self::RemovePlayer),
            5 => Some(Self::GameData),
            6 => Some(Self::GameDataTo),
            7 => Some(Self::JoinedGame),
            8 => Some(Self::EndGame),
            10 => Some(Self::AlterGame),
            11 => Some(Self::KickPlayer),
            12 => Some(Self::WaitForHost),
            13 => Some(Self::Redirect),
            14 => Some(Self::ReselectServer),
            16 => Some(Self::GetGameListV2),
            17 => Some(Self::ReportPlayer),
            18 => Some(Self::QuickMatch),
            19 => Some(Self::QuickMatchHost),
            20 => Some(Self::SetGameSession),
            21 => Some(Self::SetActivePodType),
            22 => Some(Self::QueryPlatformIds),
            23 => Some(Self::QueryLobbyInfo),
            24 => Some(Self::EndGameHostMigration),
            26 => Some(Self::PackedGameDataTo),
            _ => None,
        }
    }

    /// Returns a human-readable name for the flag.
    pub fn name(self) -> &'static str {
        match self {
            Self::HostGame => "HostGame",
            Self::JoinGame => "JoinGame",
            Self::StartGame => "StartGame",
            Self::RemoveGame => "RemoveGame",
            Self::RemovePlayer => "RemovePlayer",
            Self::GameData => "GameData",
            Self::GameDataTo => "GameDataTo",
            Self::JoinedGame => "JoinedGame",
            Self::EndGame => "EndGame",
            Self::AlterGame => "AlterGame",
            Self::KickPlayer => "KickPlayer",
            Self::WaitForHost => "WaitForHost",
            Self::Redirect => "Redirect",
            Self::ReselectServer => "ReselectServer",
            Self::GetGameListV2 => "GetGameListV2",
            Self::ReportPlayer => "ReportPlayer",
            Self::QuickMatch => "QuickMatch",
            Self::QuickMatchHost => "QuickMatchHost",
            Self::SetGameSession => "SetGameSession",
            Self::SetActivePodType => "SetActivePodType",
            Self::QueryPlatformIds => "QueryPlatformIds",
            Self::QueryLobbyInfo => "QueryLobbyInfo",
            Self::EndGameHostMigration => "EndGameHostMigration",
            Self::PackedGameDataTo => "PackedGameDataTo",
        }
    }
}
