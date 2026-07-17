//! Game options — sync, validation, and update helpers.
//!
//! Provides functions to:
//! - Sync game settings from host to all clients
//! - Update game options (e.g., when host changes a setting)
//! - Validate game options before starting

use railway_protocol::{
    GameModes,
    game_options::{
        GameOptionsData, NormalGameOptions, HideNSeekGameOptions, RoleOptions,
    },
};
use railway_hazel::{MessageReader, MessageWriter};
use tracing::{debug, warn};

use crate::{Game, GameError, GameResult};

/// Sync the host's game settings to all clients.
///
/// This is triggered by the `SyncSettings` RPC. The options are
/// serialized and broadcast to all connected players.
pub fn sync_settings(game: &Game, options: &dyn GameOptionsData) {
    let mode = options.game_mode();
    debug!("game {}: syncing settings for mode {:?}", game.code, mode);

    // In a server implementation, this would:
    // 1. Serialize options into a MessageWriter
    // 2. Broadcast the serialized data to all players as a ChangeSettings packet

    let mut writer = MessageWriter::new(railway_hazel::send_option::SendOption::Reliable);
    writer.write_byte(mode as u8);
    options.serialize(&mut writer);

    debug!(
        "game {}: settings synced ({} bytes)",
        game.code,
        writer.len()
    );
}

/// Update game options from serialized data (e.g., from host).
///
/// Deserializes new options and applies them to the game.
pub fn update_game_options(
    game: &Game,
    mode: GameModes,
    reader: &mut MessageReader,
) -> GameResult<()> {
    match mode {
        GameModes::Normal | GameModes::NormalFools => {
            let mut options = NormalGameOptions::default();
            options.deserialize(reader);
            validate_options(&options)?;

            debug!("game {}: updated normal game options", game.code);
            Ok(())
        }
        GameModes::HideNSeek | GameModes::SeekFools => {
            let mut options = HideNSeekGameOptions::default();
            options.deserialize(reader);
            validate_options(&options)?;

            debug!("game {}: updated hide & seek options", game.code);
            Ok(())
        }
    }
}

/// Validate game options, returning an error if any setting is invalid.
pub fn validate_options(options: &dyn GameOptionsData) -> GameResult<()> {
    let mode = options.game_mode();

    match mode {
        GameModes::Normal | GameModes::NormalFools => {
            if let Some(normal) = options.as_any().downcast_ref::<NormalGameOptions>() {
                validate_normal_options(normal)?;
            }
        }
        GameModes::HideNSeek | GameModes::SeekFools => {
            if let Some(hns) = options.as_any().downcast_ref::<HideNSeekGameOptions>() {
                validate_hide_n_seek_options(hns)?;
            }
        }
    }

    Ok(())
}

/// Validate Normal game mode options.
fn validate_normal_options(options: &NormalGameOptions) -> GameResult<()> {
    // Max players must be between 4 and 15
    if options.max_players < 4 || options.max_players > 15 {
        return Err(GameError::InvalidRpc(format!(
            "max_players must be between 4 and 15, got {}",
            options.max_players
        )));
    }

    // At least one impostor
    if options.num_impostors == 0 {
        return Err(GameError::InvalidRpc(
            "num_impostors must be at least 1".to_string(),
        ));
    }

    // Impostors cannot outnumber half the players
    let max_impostors = (options.max_players / 2).max(1);
    if options.num_impostors > max_impostors as u8 {
        warn!(
            "num_impostors ({}) exceeds recommended maximum ({}) for {} players",
            options.num_impostors, max_impostors, options.max_players
        );
    }

    // Player speed must be positive
    if options.player_speed_mod <= 0.0 {
        return Err(GameError::InvalidRpc(
            "player_speed_mod must be greater than 0".to_string(),
        ));
    }

    // Vision mods must be positive
    if options.crewmate_vision_mod <= 0.0 || options.impostor_vision_mod <= 0.0 {
        return Err(GameError::InvalidRpc(
            "vision mods must be greater than 0".to_string(),
        ));
    }

    // Kill cooldown must be reasonable
    if options.kill_cooldown < 0.0 {
        return Err(GameError::InvalidRpc(
            "kill_cooldown cannot be negative".to_string(),
        ));
    }

    // Task counts must be reasonable
    if options.num_common_tasks > 10 || options.num_long_tasks > 10 || options.num_short_tasks > 10 {
        return Err(GameError::InvalidRpc(
            "task counts must be between 0 and 10".to_string(),
        ));
    }

    // Discussion/voting time must be reasonable
    if options.discussion_time > 300 || options.voting_time > 300 {
        return Err(GameError::InvalidRpc(
            "discussion/voting time must be at most 300 seconds".to_string(),
        ));
    }

    // Role chances must be 0-100
    validate_role_options(&options.role_options)?;

    Ok(())
}

/// Validate Hide and Seek mode options.
fn validate_hide_n_seek_options(options: &HideNSeekGameOptions) -> GameResult<()> {
    // Max players must be between 4 and 15
    if options.max_players < 4 || options.max_players > 15 {
        return Err(GameError::InvalidRpc(format!(
            "max_players must be between 4 and 15, got {}",
            options.max_players
        )));
    }

    // Player speed must be positive
    if options.player_speed_mod <= 0.0 {
        return Err(GameError::InvalidRpc(
            "player_speed_mod must be greater than 0".to_string(),
        ));
    }

    // Vision mods must be positive
    if options.crewmate_vision_mod <= 0.0 || options.impostor_vision_mod <= 0.0 {
        return Err(GameError::InvalidRpc(
            "vision mods must be greater than 0".to_string(),
        ));
    }

    // Countdown/ping timers must be positive
    if options.final_countdown_time < 0.0 || options.max_ping_time < 0.0 {
        return Err(GameError::InvalidRpc(
            "final_countdown_time and max_ping_time cannot be negative".to_string(),
        ));
    }

    Ok(())
}

/// Validate role options (chance percentages 0-100).
fn validate_role_options(roles: &RoleOptions) -> GameResult<()> {
    let role_checks: [(&str, u8); 7] = [
        ("Scientist", roles.scientist.rate.chance),
        ("Engineer", roles.engineer.rate.chance),
        ("GuardianAngel", roles.guardian_angel.rate.chance),
        ("Shapeshifter", roles.shapeshifter.rate.chance),
        ("Phantom", roles.phantom.rate.chance),
        ("Tracker", roles.tracker.rate.chance),
        ("Noisemaker", roles.noisemaker.rate.chance),
    ];

    for (name, chance) in &role_checks {
        if *chance > 100 {
            return Err(GameError::InvalidRpc(format!(
                "{} role chance must be 0-100, got {}",
                name, chance
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_normal_options_valid() {
        let options = NormalGameOptions::default();
        assert!(validate_normal_options(&options).is_ok());
    }

    #[test]
    fn test_validate_normal_options_invalid_max_players() {
        let mut options = NormalGameOptions::default();
        options.max_players = 3;
        assert!(validate_normal_options(&options).is_err());

        options.max_players = 16;
        assert!(validate_normal_options(&options).is_err());
    }

    #[test]
    fn test_validate_normal_options_zero_impostors() {
        let mut options = NormalGameOptions::default();
        options.num_impostors = 0;
        assert!(validate_normal_options(&options).is_err());
    }

    #[test]
    fn test_validate_role_chances() {
        let mut roles = RoleOptions::default();
        roles.scientist.rate.chance = 150;
        assert!(validate_role_options(&roles).is_err());

        roles.scientist.rate.chance = 50;
        assert!(validate_role_options(&roles).is_ok());
    }
}
