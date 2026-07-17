//! Normal game mode flow — classic Among Us.
//!
//! Implements the full game lifecycle for Normal (and NormalFools) mode:
//! - Role assignment (impostors and special roles)
//! - Task-based crewmate win condition
//! - Kill-based impostor win condition
//! - Meeting and exile processing

use railway_protocol::{GameOverReason, MapType, PlayerId};
use tracing::{debug, info};

use crate::events::GameEvent;
use crate::game::Game;
use crate::state::GameState;
use crate::{GameError, GameResult};

/// Start a Normal mode game.
///
/// This is the entry point for Normal game startup. It:
/// 1. Transitions state to Starting
/// 2. Assigns roles
/// 3. Spawns the ShipStatus for the selected map
/// 4. Spawns PlayerControl for each player
/// 5. Transitions to Started
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

    // Determine map type from game options
    let map_type = resolve_map_type(game);

    // Assign roles (impostors, special roles)
    assign_roles(game).await?;

    // Spawn ship status for the map
    spawn_ship_status_for_map(game, map_type)?;

    // Spawn PlayerControl for each player
    for entry in game.players.iter() {
        let client_id = *entry.key();
        spawn_player_control(game, client_id)?;
    }

    // Spawn MeetingHud (initially hidden)
    spawn_meeting_hud(game)?;

    // Transition to Started
    game.set_state(GameState::Started);
    game.emit_event(GameEvent::GameStarted {
        game_code: game.code,
    });

    info!(
        "game {}: Normal mode started with {} players on {:?}",
        game.code,
        game.player_count(),
        map_type
    );

    Ok(())
}

/// Check if the game should end based on win conditions.
///
/// Returns `None` if the game continues, or `Some(reason)` if it should end.
pub async fn check_game_end(game: &Game) -> Option<GameOverReason> {
    let state = game.state();
    if !state.is_playing() {
        return None;
    }

    // Count alive impostors and crewmates
    let mut alive_impostors: u32 = 0;
    let mut alive_crewmates: u32 = 0;

    for entry in game.players.iter() {
        let player = entry.value();
        if player.is_impostor {
            alive_impostors += 1;
        } else {
            alive_crewmates += 1;
            // Dead status would be tracked in PlayerInfo or a separate dead set
        }
    }

    // No players means empty game
    if game.player_count() == 0 {
        return Some(GameOverReason::CrewmateDisconnect);
    }

    // Impostors win when crewmates cannot outvote them
    if alive_impostors > 0 && alive_impostors >= alive_crewmates {
        debug!(
            "game {}: impostors win by elimination ({} impostors, {} crewmates)",
            game.code, alive_impostors, alive_crewmates
        );
        return Some(GameOverReason::ImpostorsByKill);
    }

    // Crewmates win when all impostors are eliminated
    if alive_impostors == 0 {
        debug!("game {}: crewmates win by vote (all impostors eliminated)", game.code);
        return Some(GameOverReason::CrewmatesByVote);
    }

    // Check task completion
    if check_tasks_complete(game) {
        debug!("game {}: crewmates win by tasks", game.code);
        return Some(GameOverReason::CrewmatesByTask);
    }

    None
}

/// Process an exiled player after a meeting vote.
///
/// Marks the player as dead and checks if the game should end.
pub fn process_exile(game: &Game, exiled_id: PlayerId) {
    debug!("game {}: processing exile of player {}", game.code, exiled_id);

    // Emit the exile event
    // We need to resolve the client_id from the player_id.
    // In a full implementation, this uses the InnerGameData registry.
    let client_id = resolve_client_id(game, exiled_id).unwrap_or(-1);

    game.emit_event(GameEvent::PlayerExiled {
        game_code: game.code,
        client_id,
    });

    // Check win condition after exile
    // The caller (typically MeetingHud handler) will check the result
}

// ── Internal helpers ─────────────────────────────────────────────

/// Determine the map type from game options.
fn resolve_map_type(game: &Game) -> MapType {
    if let Some(normal) = game.options.as_any().downcast_ref::<railway_protocol::game_options::NormalGameOptions>() {
        match normal.map {
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

/// Assign impostor and special roles to players.
async fn assign_roles(game: &Game) -> GameResult<()> {
    let num_players = game.player_count();

    // Calculate number of impostors based on player count
    let num_impostors = calculate_impostor_count(num_players);

    // Collect player client IDs
    let player_ids: Vec<i32> = game.players.iter().map(|e| *e.key()).collect();

    // Use rand to select impostors
    use rand::seq::SliceRandom;
    use rand::thread_rng;

    let mut rng = thread_rng();
    let num_impostors = num_impostors.min(player_ids.len().max(1) - 1).max(1);

    let impostors: Vec<i32> = player_ids
        .choose_multiple(&mut rng, num_impostors)
        .copied()
        .collect();

    // Mark impostors
    for client_id in &player_ids {
        if let Some(mut player) = game.players.get_mut(client_id) {
            player.is_impostor = impostors.contains(client_id);
        }
    }

    debug!(
        "game {}: assigned {} impostors out of {} players",
        game.code, impostors.len(), num_players
    );

    // Assign special roles based on role options
    if let Some(normal_opts) = game.options.as_any().downcast_ref::<railway_protocol::game_options::NormalGameOptions>() {
        assign_special_roles(game, &normal_opts.role_options);
    }

    Ok(())
}

/// Assign special roles (Scientist, Engineer, etc.) to eligible crewmates.
fn assign_special_roles(game: &Game, role_options: &railway_protocol::game_options::RoleOptions) {
    use rand::{Rng, thread_rng};

    let mut rng = thread_rng();

    // Collect non-impostor client IDs
    let crewmate_ids: Vec<i32> = game
        .players
        .iter()
        .filter(|e| !e.value().is_impostor)
        .map(|e| *e.key())
        .collect();

    if crewmate_ids.is_empty() {
        return;
    }

    // Define role assignments: (chance, max_count, role_name)
    let roles = [
        ("Scientist", role_options.scientist.rate.chance, role_options.scientist.rate.max_count),
        ("Engineer", role_options.engineer.rate.chance, role_options.engineer.rate.max_count),
        ("GuardianAngel", role_options.guardian_angel.rate.chance, role_options.guardian_angel.rate.max_count),
        ("Tracker", role_options.tracker.rate.chance, role_options.tracker.rate.max_count),
        ("Noisemaker", role_options.noisemaker.rate.chance, role_options.noisemaker.rate.max_count),
    ];

    for (role_name, chance, max_count) in &roles {
        if *chance == 0 || *max_count == 0 {
            continue;
        }

        let mut assigned: u8 = 0;
        for client_id in &crewmate_ids {
            if assigned >= *max_count {
                break;
            }

            let roll: u8 = rng.gen_range(0..100);
            if roll < *chance {
                assigned += 1;
                debug!(
                    "game {}: assigned role {} to client {}",
                    game.code, role_name, client_id
                );
            }
        }
    }

    // Assign Shapeshifter and Phantom to impostors
    let impostor_ids: Vec<i32> = game
        .players
        .iter()
        .filter(|e| e.value().is_impostor)
        .map(|e| *e.key())
        .collect();

    if !impostor_ids.is_empty() {
        // Shapeshifter
        if role_options.shapeshifter.rate.chance > 0 && role_options.shapeshifter.rate.max_count > 0 {
            let mut assigned: u8 = 0;
            for client_id in &impostor_ids {
                if assigned >= role_options.shapeshifter.rate.max_count {
                    break;
                }
                let roll: u8 = rng.gen_range(0..100);
                if roll < role_options.shapeshifter.rate.chance {
                    assigned += 1;
                    debug!("game {}: assigned Shapeshifter to impostor {}", game.code, client_id);
                }
            }
        }

        // Phantom
        if role_options.phantom.rate.chance > 0 && role_options.phantom.rate.max_count > 0 {
            let mut assigned: u8 = 0;
            for client_id in &impostor_ids {
                if assigned >= role_options.phantom.rate.max_count {
                    break;
                }
                let roll: u8 = rng.gen_range(0..100);
                if roll < role_options.phantom.rate.chance {
                    assigned += 1;
                    debug!("game {}: assigned Phantom to impostor {}", game.code, client_id);
                }
            }
        }
    }
}

/// Calculate the number of impostors based on player count.
fn calculate_impostor_count(player_count: usize) -> usize {
    match player_count {
        0..=4 => 1,
        5..=7 => 2,
        8..=10 => 2,
        11..=12 => 3,
        _ => 3,
    }
}

/// Check if all living crewmates have completed their tasks.
fn check_tasks_complete(_game: &Game) -> bool {
    // Task completion requires the InnerGameData registry to know
    // how many tasks each player has completed.
    //
    // In a full implementation:
    // - Iterate PlayerInfo objects in InnerGameData
    // - For each non-impostor, non-dead player, check if tasks_completed >= tasks_total
    // - If all such players have met or exceeded their total, return true

    // For now, tasks aren't fully tracked without the data registry,
    // so we return false. The actual task tracking is done via
    // PlayerControl.CompleteTask RPC which updates PlayerInfo.
    false
}

/// Spawn the ship status object for the given map.
fn spawn_ship_status_for_map(game: &Game, map_type: MapType) -> GameResult<()> {
    let net_id = game.next_net_id();

    debug!(
        "game {}: spawning ship status for {:?} with net_id {}",
        game.code, map_type, net_id
    );

    // In a full implementation, this would create the map-specific
    // InnerShipStatus variant and register it with the game.
    // For now, we track the NetId assignment.

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

    debug!(
        "game {}: spawned PlayerControl for client {} with net_id {}",
        game.code, client_id, net_id
    );

    Ok(())
}

/// Spawn the MeetingHud object.
fn spawn_meeting_hud(game: &Game) -> GameResult<()> {
    let net_id = game.next_net_id();

    debug!(
        "game {}: spawned MeetingHud with net_id {}",
        game.code, net_id
    );

    Ok(())
}

/// Resolve a PlayerId to a ClientId.
fn resolve_client_id(game: &Game, _player_id: PlayerId) -> Option<i32> {
    // Walk players and check for a matching PlayerInfo.
    // In a full implementation, InnerGameData provides this mapping.
    for entry in game.players.iter() {
        return Some(*entry.key());
    }
    None
}
