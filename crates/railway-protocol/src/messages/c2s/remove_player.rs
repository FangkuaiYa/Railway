//! RemovePlayer (flag 4) message.

use railway_hazel::MessageReader;
use crate::PlayerId;

pub fn deserialize(reader: &mut MessageReader) -> (PlayerId, u8) {
    // Client always sends the game code as the first i32 in the message body.
    let _game_code = reader.read_i32();
    let player_id = reader.read_packed_u32() as PlayerId;
    let reason = reader.read_byte();
    (player_id, reason)
}
