//! RemovePlayer (flag 4) S2C message.

use railway_hazel::MessageWriter;
use crate::disconnect_reason::DisconnectReason;
use crate::message_flags::MessageFlags;
use crate::{ClientId, GameCode};

pub fn serialize(
    writer: &mut MessageWriter,
    _clear: bool,
    game_code: GameCode,
    player_id: ClientId,
    host_id: ClientId,
    reason: DisconnectReason,
) {
    writer.start_message(MessageFlags::RemovePlayer as u8);
    writer.write_i32(game_code);
    // IMPORTANT: playerId and hostId are FIXED i32, NOT packed!
    // The client reads them with ReadInt32(), not ReadPackedInt32().
    // See InnerNetClient.HandleMessage case 4:
    //   int playerIdThatLeft = reader.ReadInt32();
    //   int hostId2 = reader.ReadInt32();
    writer.write_i32(player_id);
    writer.write_i32(host_id);
    writer.write_byte(reason as u8);
    writer.end_message();
}
