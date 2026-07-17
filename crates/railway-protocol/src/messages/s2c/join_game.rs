//! JoinGame (flag 1) S2C: broadcast to existing players when someone new
//! joins the game.
//!
//! Matches C#'s `Message01JoinGameS2C.SerializeJoin`:
//! ```csharp
//! writer.Write(gameCode);          // fixed i32
//! writer.Write(player.Client.Id);  // fixed i32
//! writer.Write(hostId);            // fixed i32
//! writer.Write(player.Client.Name);
//! player.Client.PlatformSpecificData.Serialize(writer);
//! writer.WritePacked(player.Character?.PlayerInfo?.PlayerLevel ?? 1);
//! writer.Write(string.Empty); // ProductUserId
//! writer.Write(string.Empty); // FriendCode
//! ```
//!
//! Without sending this, players already in the lobby never learn about a
//! new joiner — only the joining client itself finds out (via the
//! `JoinedGame` message), so everyone else's player list/count goes stale.

use railway_hazel::MessageWriter;
use crate::message_flags::MessageFlags;
use crate::platform_data::PlatformSpecificData;
use crate::{ClientId, GameCode};

/// Serialize the JoinGame broadcast for a newly-joined player.
pub fn serialize_join(
    writer: &mut MessageWriter,
    clear: bool,
    game_code: GameCode,
    client_id: ClientId,
    host_id: ClientId,
    name: &str,
    platform_data: &PlatformSpecificData,
    player_level: i32,
) {
    if clear {
        writer.start_message(MessageFlags::JoinGame as u8);
    } else {
        writer.start_message(MessageFlags::JoinGame as u8);
    }

    writer.write_i32(game_code);
    writer.write_i32(client_id);
    writer.write_i32(host_id);
    writer.write_string(name);
    platform_data.serialize(writer);
    writer.write_packed_i32(player_level);
    writer.write_string(""); // ProductUserId — not tracked yet
    writer.write_string(""); // FriendCode — not tracked yet

    writer.end_message();
}
