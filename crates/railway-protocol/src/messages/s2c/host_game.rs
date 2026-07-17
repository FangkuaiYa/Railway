//! HostGame S2C response: sends the game code back.

use railway_hazel::MessageWriter;
use crate::message_flags::MessageFlags;
use crate::GameCode;

pub fn serialize(writer: &mut MessageWriter, game_code: GameCode) {
    writer.start_message(MessageFlags::HostGame as u8);
    writer.write_i32(game_code);
    writer.end_message();
}
