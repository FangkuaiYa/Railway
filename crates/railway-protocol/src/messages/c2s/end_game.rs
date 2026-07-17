//! EndGame (flag 8) message.

use railway_hazel::MessageReader;
use crate::GameOverReason;

pub fn deserialize(reader: &mut MessageReader) -> GameOverReason {
    // Client always sends the game code as the first i32 in the message body.
    let _game_code = reader.read_i32();
    match reader.read_byte() {
        0 => GameOverReason::CrewmatesByVote,
        1 => GameOverReason::CrewmatesByTask,
        2 => GameOverReason::ImpostorsByVote,
        3 => GameOverReason::ImpostorsByKill,
        4 => GameOverReason::ImpostorsBySabotage,
        5 => GameOverReason::ImpostorDisconnect,
        6 => GameOverReason::CrewmateDisconnect,
        7 => GameOverReason::HideAndSeekByTimer,
        8 => GameOverReason::HideAndSeekByKills,
        _ => GameOverReason::CrewmatesByVote,
    }
}
