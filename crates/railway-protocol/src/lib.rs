//! # railway-protocol
//!
//! Among Us protocol definitions: message flags, RPC calls, game options,
//! and serialization/deserialization of all client-server messages.
//!
//! This crate depends on `railway-hazel` for the low-level message reading/writing.

pub mod disconnect_reason;
pub mod game_data_tag;
pub mod game_options;
pub mod game_version;
pub mod message_flags;
pub mod platform_data;
pub mod rpc_calls;

pub mod messages;

pub use disconnect_reason::DisconnectReason;
pub use game_data_tag::GameDataTag;
pub use game_version::GameVersion;
pub use message_flags::MessageFlags;
pub use platform_data::PlatformSpecificData;
pub use rpc_calls::RpcCalls;

/// Among Us game language identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Language {
    English = 0,
    Spanish = 1,
    Portuguese = 2,
    Korean = 3,
    Russian = 4,
    French = 5,
    German = 6,
    Italian = 7,
    Japanese = 8,
    ChineseSimplified = 9,
    ChineseTraditional = 10,
    Irish = 11,
}

/// Quick chat mode flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum QuickChatModes {
    FreeChatOrQuickChat = 0,
    QuickChatOnly = 1,
}

/// Player color types in Among Us.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ColorType {
    Red = 0,
    Blue = 1,
    Green = 2,
    Pink = 3,
    Orange = 4,
    Yellow = 5,
    Black = 6,
    White = 7,
    Purple = 8,
    Brown = 9,
    Cyan = 10,
    Lime = 11,
    Maroon = 12,
    Rose = 13,
    Banana = 14,
    Gray = 15,
    Tan = 16,
    Coral = 17,
    // Additional colors may be added in newer versions
}

/// Role types in Among Us.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum RoleTypes {
    Crewmate = 0,
    Impostor = 1,
    Scientist = 2,
    Engineer = 3,
    GuardianAngel = 4,
    Shapeshifter = 5,
    CrewmateGhost = 6,
    ImpostorGhost = 7,
    Noisemaker = 8,
    Phantom = 9,
    Tracker = 10,
    // 11 is unused/reserved
    Detective = 12,
    // 13–17 are unused/reserved
    Viper = 18,
}

/// Game modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GameModes {
    Normal = 0,
    HideNSeek = 1,
    NormalFools = 2,
    SeekFools = 3,
}

/// Map types (used by the game).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MapType {
    Skeld = 0,
    MiraHQ = 1,
    Polus = 2,
    Dleks = 3,
    Airship = 4,
    Fungle = 5,
}

/// Game over reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GameOverReason {
    CrewmatesByVote = 0,
    CrewmatesByTask = 1,
    ImpostorsByVote = 2,
    ImpostorsByKill = 3,
    ImpostorsBySabotage = 4,
    ImpostorDisconnect = 5,
    CrewmateDisconnect = 6,
    HideAndSeekByTimer = 7,
    HideAndSeekByKills = 8,
}

/// Murder result flags sent with the MurderPlayer RPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MurderResultFlags(u32);

impl MurderResultFlags {
    pub const NONE: Self = Self(0);
    pub const SUCCEEDED: Self = Self(0x01);
    pub const FAILED: Self = Self(0x02);

    /// Create from raw u32 bits.
    pub fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    pub fn succeeded(self) -> bool {
        self.0 & Self::SUCCEEDED.0 != 0
    }
}

/// Cross-platform play flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum CrossplayFlags {
    None = 0,
    Mobile = 1,
    PC = 2,
    Console = 4,
}

/// Alter game tags (for the AlterGame message).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AlterGameTags {
    ChangePrivacy = 1,
}

/// Spawn flags for InnerNetObjects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnFlags(u8);

impl SpawnFlags {
    pub const NONE: Self = Self(0);
    pub const IS_CLIENT_CHARACTER: Self = Self(1);

    pub fn is_client_character(self) -> bool {
        self.0 & Self::IS_CLIENT_CHARACTER.0 != 0
    }
}

/// A game code (4-6 character code, stored as an i32).
pub type GameCode = i32;

/// Network ID for game objects.
pub type NetId = u32;

/// Client/player identifier.
pub type ClientId = i32;

/// Player slot identifier (0-14).
pub type PlayerId = u8;

/// Special owner IDs.
pub const HOST_INHERIT_ID: ClientId = -2;
pub const CURRENT_CLIENT_ID: ClientId = -3;
pub const SERVER_OWNED_ID: ClientId = -4;

/// The first NetId reserved for server-owned objects.
pub const MIN_SERVER_NET_ID: NetId = 100_000;
