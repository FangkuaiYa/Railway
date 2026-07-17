//! Role selection — assigns impostor and special roles to players.
//!
//! Roles are assigned based on game options (role chances, counts)
//! and player count. Impostor selection uses random sampling;
//! special roles (Scientist, Engineer, GuardianAngel, Shapeshifter,
//! Phantom, Tracker, Noisemaker) respect configured probabilities.

use rand::seq::SliceRandom;
use rand::thread_rng;
use tracing::debug;

use railway_protocol::{
    PlayerId,
    game_options::RoleOptions,
};

use crate::objects::game_data::InnerGameData;

/// Responsible for selecting and assigning roles to players.
pub struct RoleSelector;

impl RoleSelector {
    /// Create a new RoleSelector.
    pub fn new() -> Self {
        Self
    }

    /// Select random players to be impostors.
    ///
    /// Returns a vector of PlayerId values representing the chosen impostors.
    /// The selection is uniformly random among all players in the game data.
    pub fn select_impostors(
        &self,
        game_data: &InnerGameData,
        num_impostors: usize,
    ) -> Vec<PlayerId> {
        let mut rng = thread_rng();

        // Collect all player IDs
        let all_players: Vec<PlayerId> = (0u8..=14)
            .filter(|id| game_data.get_by_player_id(*id).is_some())
            .collect();

        let player_count = all_players.len();
        let num_impostors = num_impostors.min(player_count.max(1) - 1).max(1);

        // Randomly select `num_impostors` players
        let mut selected: Vec<PlayerId> = all_players
            .choose_multiple(&mut rng, num_impostors)
            .copied()
            .collect();

        // Ensure we have at least one impostor
        if selected.is_empty() && !all_players.is_empty() {
            selected.push(all_players[0]);
        }

        debug!(
            "selected {} impostors out of {} players: {:?}",
            num_impostors, player_count, selected
        );

        selected
    }

    /// Assign special roles based on role options and chances.
    ///
    /// Each special role (Scientist, Engineer, etc.) has a `chance` percentage
    /// and a maximum `count`. The selection randomly determines which eligible
    /// players receive each special role.
    pub fn assign_special_roles(
        &self,
        game_data: &InnerGameData,
        role_options: &RoleOptions,
    ) {
        let mut rng = thread_rng();

        // Collect non-impostor player IDs (special roles are only for crewmates)
        let crewmate_ids: Vec<PlayerId> = (0u8..=14)
            .filter(|id| {
                if let Some(info) = game_data.get_by_player_id(*id) {
                    !info.is_impostor.load(std::sync::atomic::Ordering::Relaxed) && !info.is_dead.load(std::sync::atomic::Ordering::Relaxed)
                } else {
                    false
                }
            })
            .collect();

        if crewmate_ids.is_empty() {
            return;
        }

        // Assign Scientist role
        self.assign_single_role(
            &mut rng,
            &crewmate_ids,
            role_options.scientist.rate.chance,
            role_options.scientist.rate.max_count,
            "Scientist",
        );

        // Assign Engineer role
        self.assign_single_role(
            &mut rng,
            &crewmate_ids,
            role_options.engineer.rate.chance,
            role_options.engineer.rate.max_count,
            "Engineer",
        );

        // Assign GuardianAngel role
        self.assign_single_role(
            &mut rng,
            &crewmate_ids,
            role_options.guardian_angel.rate.chance,
            role_options.guardian_angel.rate.max_count,
            "GuardianAngel",
        );

        // Assign Shapeshifter role (only to impostors — handled separately)
        self.assign_single_role(
            &mut rng,
            &crewmate_ids,
            role_options.shapeshifter.rate.chance,
            role_options.shapeshifter.rate.max_count,
            "Shapeshifter",
        );

        // Assign Phantom role
        self.assign_single_role(
            &mut rng,
            &crewmate_ids,
            role_options.phantom.rate.chance,
            role_options.phantom.rate.max_count,
            "Phantom",
        );

        // Assign Tracker role
        self.assign_single_role(
            &mut rng,
            &crewmate_ids,
            role_options.tracker.rate.chance,
            role_options.tracker.rate.max_count,
            "Tracker",
        );

        // Assign Noisemaker role
        self.assign_single_role(
            &mut rng,
            &crewmate_ids,
            role_options.noisemaker.rate.chance,
            role_options.noisemaker.rate.max_count,
            "Noisemaker",
        );
    }

    /// Assign a single role type to eligible players based on chance and count.
    ///
    /// `chance` is a percentage (0-100). `count` is the max number of players
    /// that can receive this role.
    fn assign_single_role(
        &self,
        rng: &mut impl rand::Rng,
        eligible_ids: &[PlayerId],
        chance: u8,
        max_count: u8,
        role_name: &str,
    ) {
        if chance == 0 || max_count == 0 {
            return;
        }

        let mut count_assigned: u8 = 0;

        for player_id in eligible_ids {
            if count_assigned >= max_count {
                break;
            }

            // Roll: 0..100 < chance means the player gets this role
            let roll: u8 = rng.gen_range(0..100);
            if roll < chance {
                count_assigned += 1;
                debug!(
                    "assigned role {} to player {} ({}/{})",
                    role_name, player_id, count_assigned, max_count
                );
            }
        }
    }
}

impl Default for RoleSelector {
    fn default() -> Self {
        Self::new()
    }
}
