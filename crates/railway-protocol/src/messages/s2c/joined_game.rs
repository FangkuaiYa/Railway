//! JoinedGame (flag 7) S2C: sent when a player joins a game.
//!
//! IMPORTANT — this must match C#'s `Message07JoinedGameS2C.Serialize`
//! exactly:
//! ```csharp
//! writer.Write(gameCode);              // fixed i32
//! writer.Write(playerId);              // fixed i32 (NOT packed!)
//! writer.Write(hostId);                // fixed i32 (NOT packed!)
//! writer.WritePacked(otherPlayers.Length);
//! foreach (var ply in otherPlayers) {
//!     writer.WritePacked(ply.Client.Id);
//!     writer.Write(ply.Client.Name);
//!     ply.Client.PlatformSpecificData.Serialize(writer);
//!     writer.WritePacked(ply.Character?.PlayerInfo?.PlayerLevel ?? 1);
//!     writer.Write(string.Empty); // ProductUserId
//!     writer.Write(string.Empty); // FriendCode
//! }
//! ```
//! A previous version of this file invented a totally different per-player
//! layout (color/hat/pet/skin/visor/nameplate/tasksDone/isImpostor) that
//! doesn't exist anywhere in this message — that data is synced separately
//! via GameData/RPC once players spawn. Sending the wrong layout here
//! desyncs the joining client's reader immediately, which is why the
//! client got stuck on "waiting for host" with garbage player counts/state.

use railway_hazel::MessageWriter;
use crate::message_flags::MessageFlags;
use crate::platform_data::PlatformSpecificData;
use crate::{ClientId, GameCode};

/// Serialize the JoinedGame message containing all current players.
pub fn serialize_join(
    writer: &mut MessageWriter,
    clear: bool,
    game_code: GameCode,
    client_id: ClientId,
    host_id: ClientId,
    players: &[JoinedPlayerInfo],
) {
    if clear {
        writer.start_message(MessageFlags::JoinedGame as u8);
    } else {
        writer.start_message(MessageFlags::JoinedGame as u8);
    }

    writer.write_i32(game_code);
    writer.write_i32(client_id);
    writer.write_i32(host_id);

    writer.write_packed_u32(players.len() as u32);
    for p in players {
        writer.write_packed_i32(p.client_id);
        writer.write_string(&p.name);
        p.platform_data.serialize(writer);
        writer.write_packed_i32(p.player_level);
        writer.write_string(""); // ProductUserId — not tracked yet
        writer.write_string(""); // FriendCode — not tracked yet
    }

    writer.end_message();
}

/// Info about a player in the JoinedGame message.
#[derive(Debug, Clone)]
pub struct JoinedPlayerInfo {
    pub client_id: ClientId,
    pub name: String,
    pub platform_data: PlatformSpecificData,
    /// Cosmetic "player level" shown next to the name (defaults to 1 if unknown).
    pub player_level: i32,
}
