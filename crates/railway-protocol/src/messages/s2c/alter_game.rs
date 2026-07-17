//! AlterGame (flag 10) S2C message.

use railway_hazel::MessageWriter;
use crate::message_flags::MessageFlags;
use crate::GameCode;

pub fn serialize(writer: &mut MessageWriter, _clear: bool, game_code: GameCode, is_public: bool) {
    writer.start_message(MessageFlags::AlterGame as u8);
    writer.write_i32(game_code);
    writer.write_byte(1); // tag: ChangePrivacy
    writer.write_bool(is_public);
    writer.end_message();
}
