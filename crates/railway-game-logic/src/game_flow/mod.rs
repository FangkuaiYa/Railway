//! Game flow logic — orchestrates game lifecycle events.
//!
//! Handles:
//! - Starting the game (role assignment, map setup)
//! - Ending the game (win conditions, voting results)
//! - Meeting lifecycle (start, voting, end, exile)
//! - Role selection (impostors, special roles)
//! - Options validation and syncing
//! - Usable items (consoles, admin table, vitals, etc.)
//!
//! The `GameFlow` struct is the main orchestrator, while the `normal`
//! and `hide_and_seek` modules implement mode-specific logic.

pub mod game_flow;
pub mod role_selection;
pub mod options;
pub mod usables;
pub mod normal;
pub mod hide_and_seek;

pub use game_flow::GameFlow;
pub use role_selection::RoleSelector;

use railway_protocol::GameOverReason;
use crate::events::GameEvent;
use crate::game::Game;
use crate::state::GameState;

/// Start a game: assign roles, spawn objects, transition state.
///
/// Dispatches to the appropriate mode-specific implementation
/// based on the game's configured mode.
pub async fn start_game(game: &Game) -> crate::GameResult<()> {
    let mode = game.options.game_mode();
    match mode {
        railway_protocol::GameModes::Normal | railway_protocol::GameModes::NormalFools => {
            normal::start_game(game).await
        }
        railway_protocol::GameModes::HideNSeek | railway_protocol::GameModes::SeekFools => {
            hide_and_seek::start_game(game).await
        }
    }
}

/// End a game: clean up objects, transition state.
pub async fn end_game(game: &Game, reason: GameOverReason) {
    game.set_state(GameState::Ended);
    game.emit_event(GameEvent::GameEnded {
        game_code: game.code,
        reason,
    });
}

/// Handle a meeting start.
pub async fn start_meeting(game: &Game) {
    game.emit_event(GameEvent::MeetingStarted {
        game_code: game.code,
    });
}

/// Handle a meeting end.
pub async fn end_meeting(game: &Game) {
    game.emit_event(GameEvent::MeetingEnded {
        game_code: game.code,
    });
}
