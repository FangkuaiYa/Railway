//! Platform-specific data sent by the client during handshake.

use railway_hazel::MessageReader;
use railway_hazel::MessageWriter;

/// Platform-specific identifiers sent by the client.
///
/// These are used for cross-platform play and friend features.
#[derive(Debug, Clone)]
pub struct PlatformSpecificData {
    /// The platform the client is running on.
    pub platform: Platform,
    /// Display name of the platform account.
    pub platform_name: String,
    /// Xbox Live user ID — only present when `platform == Xbox`.
    pub xbox_platform_id: Option<u64>,
    /// PlayStation Network user ID — only present when `platform == Playstation`.
    pub psn_platform_id: Option<u64>,
}

impl PlatformSpecificData {
    /// Deserialize from a Hazel message reader.
    ///
    /// IMPORTANT — this matches C#'s `PlatformSpecificData(IMessageReader reader)`:
    /// - `platform` comes from the sub-message's **tag byte** (`reader.tag`),
    ///   it is NOT a packed u32 read from the payload.
    /// - `platform_name`: a single string.
    /// - `xbox_platform_id`: u64, present ONLY if platform == Xbox.
    /// - `psn_platform_id`: u64, present ONLY if platform == Playstation.
    ///   There is no "friend_code"/"client_id" pair in this message at all —
    ///   reading those (as the previous implementation did) desyncs the
    ///   reader position for everything that follows in the handshake,
    ///   which is what produced garbage string lengths (e.g. panics like
    ///   "range end index N out of range") a few reads later.
    pub fn deserialize(reader: &mut MessageReader) -> Self {
        let platform = Platform::from_u32(reader.tag as u32);
        let platform_name = reader.read_string();

        let xbox_platform_id = if platform == Platform::Xbox {
            Some(reader.read_u64())
        } else {
            None
        };
        let psn_platform_id = if platform == Platform::Playstation {
            Some(reader.read_u64())
        } else {
            None
        };

        Self {
            platform,
            platform_name,
            xbox_platform_id,
            psn_platform_id,
        }
    }

    /// Serialize to a Hazel message writer.
    ///
    /// Matches C#'s `Serialize`: the platform is written as the sub-message
    /// tag via `start_message`, not as a payload field.
    pub fn serialize(&self, writer: &mut MessageWriter) {
        writer.start_message(self.platform.to_u32() as u8);
        writer.write_string(&self.platform_name);
        match self.platform {
            Platform::Xbox => {
                writer.write_u64(
                    self.xbox_platform_id
                        .expect("xbox_platform_id must be set when platform is Xbox"),
                );
            }
            Platform::Playstation => {
                writer.write_u64(
                    self.psn_platform_id
                        .expect("psn_platform_id must be set when platform is Playstation"),
                );
            }
            _ => {}
        }
        writer.end_message();
    }
}

/// Client platform identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Platform {
    Unknown = 0,
    StandaloneItch = 1,
    StandaloneWin10 = 2,
    StandaloneSteam = 3,
    StandaloneEpic = 4,
    StandaloneMac = 5,
    Android = 6,
    IOS = 7,
    Xbox = 8,
    Playstation = 9,
    Switch = 10,
}

impl Platform {
    pub fn from_u32(value: u32) -> Self {
        match value {
            1 => Self::StandaloneItch,
            2 => Self::StandaloneWin10,
            3 => Self::StandaloneSteam,
            4 => Self::StandaloneEpic,
            5 => Self::StandaloneMac,
            6 => Self::Android,
            7 => Self::IOS,
            8 => Self::Xbox,
            9 => Self::Playstation,
            10 => Self::Switch,
            _ => Self::Unknown,
        }
    }

    pub fn to_u32(self) -> u32 {
        self as u32
    }
}

/// Runtime platform (Unity runtime platform identifiers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RuntimePlatform {
    Unknown = 0,
    WindowsPlayer = 2,
    OSXPlayer = 3,
    LinuxPlayer = 13,
    IPhonePlayer = 9,
    AndroidPlayer = 11,
    SwitchPlayer = 32,
    XboxOne = 27,
    PS4 = 25,
    PS5 = 38,
}

impl RuntimePlatform {
    pub fn from_u32(value: u32) -> Self {
        match value {
            2 => Self::WindowsPlayer,
            3 => Self::OSXPlayer,
            13 => Self::LinuxPlayer,
            9 => Self::IPhonePlayer,
            11 => Self::AndroidPlayer,
            32 => Self::SwitchPlayer,
            27 => Self::XboxOne,
            25 => Self::PS4,
            38 => Self::PS5,
            _ => Self::Unknown,
        }
    }
}
