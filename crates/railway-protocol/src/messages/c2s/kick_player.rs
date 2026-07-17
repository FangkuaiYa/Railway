//! KickPlayer (flag 11) message.

use railway_hazel::MessageReader;
use crate::ClientId;

pub fn deserialize(reader: &mut MessageReader) -> (ClientId, bool) {
    // Client always sends the game code as the first i32 in the message body.
    let _game_code = reader.read_i32();
    // IMPORTANT: despite the field being named "playerId" in the real C#
    // source (`Message11KickPlayerC2S.Deserialize`), it's actually a full
    // `ClientId` (signed, packed i32) — the value used to look up which
    // network CONNECTION to disconnect, not the small 0-255 in-game
    // PlayerId slot. A previous version read this as `PlayerId` (u8),
    // silently truncating any client id above 255 down to its low byte —
    // which would kick/ban the WRONG connection (or a nonexistent one)
    // for any real deployment where client ids grow past 255.
    let client_id = reader.read_packed_i32();
    let is_ban = reader.read_bool();
    (client_id, is_ban)
}
