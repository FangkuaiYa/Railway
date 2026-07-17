//! WaitForHost (flag 12) S2C message.

use railway_hazel::MessageWriter;
use crate::message_flags::MessageFlags;
use crate::{ClientId, GameCode};

pub fn serialize(writer: &mut MessageWriter, _clear: bool, game_code: GameCode, client_id: ClientId) {
    writer.start_message(MessageFlags::WaitForHost as u8);
    writer.write_i32(game_code);
    // IMPORTANT: client_id is a FIXED i32, NOT packed!
    // The client reads it with ReadInt32() (see InnerNetClient.HandleMessage IL_332).
    writer.write_i32(client_id);
    writer.end_message();
}
