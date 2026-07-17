//! Tags used within GameData messages to identify the type of sub-message.

/// Each GameData message contains sub-messages identified by these tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GameDataTag {
    /// Serialization data update for an existing object.
    DataFlag = 1,
    /// Remote procedure call on an object.
    RpcFlag = 2,
    /// Spawn a new game object.
    SpawnFlag = 4,
    /// Despawn (destroy) an existing game object.
    DespawnFlag = 5,
    /// Client is changing scenes (map loading).
    SceneChangeFlag = 6,
    /// Client is ready.
    ReadyFlag = 7,
    /// Game settings changed.
    ChangeSettingsFlag = 8,
    /// Console client declaring its platform type.
    ConsoleDeclareClientPlatformFlag = 205,
    /// PS4 room request.
    PS4RoomRequestFlag = 206,
}

impl GameDataTag {
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::DataFlag),
            2 => Some(Self::RpcFlag),
            4 => Some(Self::SpawnFlag),
            5 => Some(Self::DespawnFlag),
            6 => Some(Self::SceneChangeFlag),
            7 => Some(Self::ReadyFlag),
            8 => Some(Self::ChangeSettingsFlag),
            205 => Some(Self::ConsoleDeclareClientPlatformFlag),
            206 => Some(Self::PS4RoomRequestFlag),
            _ => None,
        }
    }
}
