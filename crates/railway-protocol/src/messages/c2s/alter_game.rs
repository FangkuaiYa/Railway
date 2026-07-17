//! AlterGame (flag 10) message.

use railway_hazel::MessageReader;
use crate::AlterGameTags;

pub fn deserialize(reader: &mut MessageReader) -> (AlterGameTags, bool) {
    // Client always sends the game code as the first i32 in the message body.
    // Consume it so the remaining reads align with the actual tag/value fields.
    let _game_code = reader.read_i32();
    let tag = match reader.read_byte() {
        1 => AlterGameTags::ChangePrivacy,
        _ => AlterGameTags::ChangePrivacy, // default
    };
    let value = reader.read_bool();
    (tag, value)
}
