//! Reactor mod protocol parser.
//!
//! Reactor-modded Among Us clients append extra data after the vanilla
//! handshake fields. The format is:
//!
//! ```text
//! ReactorHeader (8 bytes, UInt64 LE):
//!   [0..6] magic: 0x72656163746f72 ("reactor" in ASCII, 7 bytes)
//!   [7]    protocol_version: u8 (1 = V2, 2 = V3)
//!
//! IF protocol_version >= 2 (V3):
//!   [packed_u32] mod_count
//!   For each mod:
//!     [string]  id         — unique mod identifier (e.g. "gg.reactor.Example")
//!     [string]  version    — mod version string
//!     [u16 LE]  flags      — ModFlags bitmask
//!     [string]? name       — human-readable name (only if flags & 1)
//! ```
//!
//! ModFlags (u16):
//!   0x01 = RequireOnAllClients
//!   0x02 = RequireOnServer
//!   0x04 = RequireOnHost
//!   0x08 = DisableServerAuthority

use railway_hazel::message::MessageReader;
use bytes::Bytes;

/// Magic bytes for the Reactor header: "reactor" in ASCII.
const REACTOR_MAGIC: u64 = 0x72656163746f72;

/// Minimum header size: 8 bytes (magic + version).
const HEADER_SIZE: usize = 8;

/// A parsed Reactor mod entry.
#[derive(Debug, Clone)]
pub struct ReactorMod {
    /// Unique mod identifier (e.g. "gg.reactor.Example").
    pub id: String,
    /// Mod version string.
    pub version: String,
    /// Mod flags bitmask.
    pub flags: u16,
    /// Human-readable name (only present when RequireOnAllClients is set).
    pub name: Option<String>,
}

impl ReactorMod {
    /// Returns true if this mod requires all clients to have it.
    pub fn is_required_on_all_clients(&self) -> bool {
        (self.flags & 0x01) != 0
    }

    /// Returns true if this mod requires the server to support it.
    pub fn is_required_on_server(&self) -> bool {
        (self.flags & 0x02) != 0
    }
}

/// Parsed Reactor handshake data.
#[derive(Debug, Clone)]
pub struct ReactorHandshake {
    /// Reactor protocol version (1 = V2, 2 = V3).
    pub protocol_version: u8,
    /// List of mods installed by the client.
    pub mods: Vec<ReactorMod>,
}

/// Try to parse a Reactor mod handshake from trailing handshake data.
///
/// Returns `None` if:
/// - The data is too short (less than 8 bytes)
/// - The magic bytes don't match "reactor"
/// - The protocol version is unsupported (0 or > 2)
pub fn parse_reactor_handshake(trailing_data: &[u8]) -> Option<ReactorHandshake> {
    if trailing_data.len() < HEADER_SIZE {
        return None;
    }

    // Read the 8-byte UInt64 header (little-endian).
    let header_bytes: [u8; 8] = trailing_data[..8].try_into().ok()?;
    let header_value = u64::from_le_bytes(header_bytes);
    let magic = header_value >> 8;
    let protocol_version = (header_value & 0xFF) as u8;

    if magic != REACTOR_MAGIC {
        return None; // Not a Reactor client
    }

    if protocol_version < 2 {
        // V2 (protocol_version=1) doesn't include mod list
        return Some(ReactorHandshake {
            protocol_version,
            mods: Vec::new(),
        });
    }

    // Parse the mod list from remaining bytes
    let mod_data = &trailing_data[HEADER_SIZE..];
    let mut reader = MessageReader::new(Bytes::copy_from_slice(mod_data), 0);

    let mod_count = reader.read_packed_u32() as usize;
    let mut mods = Vec::with_capacity(mod_count.min(64)); // safety cap

    for _ in 0..mod_count {
        let id = reader.read_string();
        let version = reader.read_string();
        let flags = reader.read_u16();
        let name = if (flags & 0x01) != 0 {
            Some(reader.read_string())
        } else {
            None
        };

        mods.push(ReactorMod {
            id,
            version,
            flags,
            name,
        });
    }

    Some(ReactorHandshake {
        protocol_version,
        mods,
    })
}
