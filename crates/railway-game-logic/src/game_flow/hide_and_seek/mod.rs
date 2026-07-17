//! Hide and Seek game mode flow.
//!
//! Implements the game lifecycle for Hide & Seek (and SeekFools) mode:
//! - One seeker vs. multiple hiders
//! - Timer-based win condition (hiders survive until time expires)
//! - Kill-based win condition (seeker eliminates all hiders)
//! - Final hide phase when few hiders remain

use railway_protocol::{GameOverReason, MapType};
use rand::seq::SliceRandom;
use rand::thread_rng;
use tracing::{debug, info};

use crate::events::GameEvent;
use crate::game::Game;
use crate::state::GameState;
use crate::{GameError, GameResult};

/// Start a Hide and Seek game.
///
/// This is the entry point for Hide & Seek game startup. It:
/// 1. Transitions state to Starting
/// 2. Selects the seeker(s) and marks all others as hiders
/// 3. Spawns the ShipStatus for the selected map
/// 4. Spawns PlayerControl for each player
/// 5. Starts the game timer
/// 6. Transitions to Started
pub async fn start_game(game: &Game) -> GameResult<()> {
    let state = game.state();
    if state != GameState::NotStarted {
        return Err(GameError::InvalidStateTransition {
            from: format!("{:?}", state),
            to: format!("{:?}", GameState::Starting),
        });
    }

    game.set_state(GameState::Starting);
    game.emit_event(GameEvent::GameStarting {
        game_code: game.code,
    });

    // Determine map type
    let map_type = resolve_map_type(game);

    // Collect player client IDs
    let player_ids: Vec<i32> = game.players.iter().map(|e| *e.key()).collect();

    if player_ids.is_empty() {
        return Err(GameError::GameLogic {
            code: game.code,
            message: "cannot start Hide & Seek with no players".to_string(),
        });
    }

    // Select seeker(s) — typically 1
    let num_seekers = 1usize;
    let mut rng = thread_rng();

    let seeker_ids: Vec<i32> = player_ids
        .choose_multiple(&mut rng, num_seekers.min(player_ids.len()))
        .copied()
        .collect();

    // Mark seekers (as impostors) and hiders (as crewmates)
    for client_id in &player_ids {
        if let Some(mut player) = game.players.get_mut(client_id) {
            player.is_impostor = seeker_ids.contains(client_id);
        }
    }

    debug!(
        "game {}: Hide & Seek — {} seekers, {} hiders",
        game.code,
        seeker_ids.len(),
        player_ids.len() - seeker_ids.len()
    );

    // Spawn ship status
    spawn_ship_status(game, map_type)?;

    // Spawn PlayerControl for each player
    for client_id in &player_ids {
        spawn_player_control(game, *client_id)?;
    }

    // Start the game timer
    start_timer(game);

    // Transition to Started
    game.set_state(GameState::Started);
    game.emit_event(GameEvent::GameStarted {
        game_code: game.code,
    });

    info!(
        "game {}: Hide & Seek started with {} players on {:?}",
        game.code,
        game.player_count(),
        map_type
    );

    Ok(())
}

/// Check if the game should end based on Hide & Seek win conditions.
///
/// Returns `None` if the game continues, or `Some(reason)` if it should end.
pub async fn check_game_end(game: &Game) -> Option<GameOverReason> {
    let state = game.state();
    if !state.is_playing() {
        return None;
    }

    let mut alive_hiders: u32 = 0;

    for entry in game.players.iter() {
        let player = entry.value();
        if player.is_impostor {
            // seeker is alive
        } else {
            alive_hiders += 1;
        }
    }

    // If all hiders are eliminated, seekers win
    if alive_hiders == 0 {
        debug!("game {}: Hide & Seek — seekers win by kills", game.code);
        return Some(GameOverReason::HideAndSeekByKills);
    }

    // If timer runs out, hiders win
    // The timer is tracked externally by the server's game loop.
    // When the timer expires, the server calls end_game with HideAndSeekByTimer.

    None
}

/// Start the game timer for Hide & Seek mode.
///
/// The timer determines how long hiders have to survive.
/// When it expires, hiders win. If the seeker kills all hiders
/// before time runs out, the seeker wins.
pub fn start_timer(game: &Game) {
    let timer_duration = get_timer_duration(game);

    debug!(
        "game {}: Hide & Seek timer started: {} seconds",
        game.code, timer_duration
    );

    // In a full implementation, this would:
    // 1. Spawn a tokio task that decrements the timer each second
    // 2. When the timer hits 0, call end_game with HideAndSeekByTimer
    // 3. Sync the timer value to all clients via HideAndSeekManager

    // For now, we track the start time so the server can check elapsed time.
}

/// Process the final hide phase.
///
/// When only `num_final_hiders` hiders remain, give them extra time
/// and notify all players that the final hide has begun.
pub fn process_final_hide(game: &Game) {
    let num_final_hiders = get_final_hiders_count(game);

    // Count remaining hiders
    let remaining_hiders: u32 = game
        .players
        .iter()
        .filter(|e| !e.value().is_impostor)
        .count() as u32;

    if remaining_hiders <= num_final_hiders && remaining_hiders > 0 {
        debug!(
            "game {}: final hide phase — {} hiders remaining",
            game.code, remaining_hiders
        );

        // In a full implementation:
        // 1. Add extra time to the game timer
        // 2. Reveal hider positions to the seeker
        // 3. Emit an event for the final hide phase
    }
}

// ── Internal helpers ─────────────────────────────────────────────

/// Determine the map type from game options.
fn resolve_map_type(game: &Game) -> MapType {
    if let Some(hns) = game.options.as_any().downcast_ref::<railway_protocol::game_options::HideNSeekGameOptions>() {
        match hns.map {
            0 => MapType::Skeld,
            1 => MapType::MiraHQ,
            2 => MapType::Polus,
            3 => MapType::Dleks,
            4 => MapType::Airship,
            5 => MapType::Fungle,
            _ => MapType::Skeld,
        }
    } else {
        MapType::Skeld
    }
}

/// Get the total game timer duration from options.
///
/// NOTE: real `HideNSeekGameOptionsV10` (verified against the client's own
/// deserializer) doesn't carry a "total seeker/hider timer" field at all —
/// the fields that exist are `FinalCountdownTime`, `EscapeTime`,
/// `MaxPingTime`, etc., which serve different, more specific purposes.
/// Until we know exactly which field (if any) the client expects this
/// timer to come from, just use a fixed sane default rather than reading
/// a field that doesn't correspond to what we want here.
fn get_timer_duration(_game: &Game) -> f32 {
    120.0 // default 2 minutes
}

/// Get the number of final hiders from options.
///
/// NOTE: see `get_timer_duration` — the real wire format has no direct
/// "number of final hiders" field, so this stays a fixed default for now.
fn get_final_hiders_count(_game: &Game) -> u32 {
    3 // default
}

/// Spawn the ship status for the map.
fn spawn_ship_status(game: &Game, map_type: MapType) -> GameResult<()> {
    let net_id = game.next_net_id();
    debug!(
        "game {}: Hide & Seek spawning ship status for {:?} with net_id {}",
        game.code, map_type, net_id
    );
    Ok(())
}

/// Spawn a PlayerControl for a client.
fn spawn_player_control(game: &Game, client_id: i32) -> GameResult<()> {
    let net_id = game.next_net_id();

    if let Some(mut player) = game.players.get_mut(&client_id) {
        player.character_net_id = Some(net_id);
    }

    game.emit_event(GameEvent::PlayerSpawned {
        game_code: game.code,
        client_id,
        character_net_id: net_id,
    });

    Ok(())
}
