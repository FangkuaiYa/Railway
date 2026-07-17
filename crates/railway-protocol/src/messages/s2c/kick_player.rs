//! KickPlayer (flag 11) S2C message.

use railway_hazel::MessageWriter;
use crate::message_flags::MessageFlags;
use crate::{ClientId, GameCode};

pub fn serialize(
    writer: &mut MessageWriter,
    _clear: bool,
    game_code: GameCode,
    player_id: ClientId,
    is_ban: bool,
) {
    writer.start_message(MessageFlags::KickPlayer as u8);
    writer.write_i32(game_code);
    writer.write_packed_i32(player_id);
    writer.write_bool(is_ban);
    writer.end_message();
}
