//! Concrete anti-cheat check functions.
//!
//! Every function in this module is a pure function that returns a
//! [`CheatResult`](super::CheatResult). They do not mutate state or perform I/O.

use railway_protocol::{ClientId, RoleTypes};

use super::CheatResult;
use crate::state::GameState;

// ── Packet & name validation ───────────────────────────────────────

/// Reject packets that exceed the protocol size limit.
pub fn check_packet_size(size: usize, limit: usize) -> CheatResult {
    if size > limit {
        CheatResult::Cheat {
            message: format!(
                "packet size {} exceeds maximum allowed size of {} bytes",
                size, limit
            ),
        }
    } else {
        CheatResult::Allow
    }
}

/// Reject names that are empty, whitespace-only, or longer than 10 characters.
///
/// Among Us enforces a 10-character display-name limit. Names that are all
/// whitespace are also rejected because they render invisibly.
pub fn check_player_name(name: &str) -> CheatResult {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return CheatResult::Cheat {
            message: "player name is empty or contains only whitespace".into(),
        };
    }
    if trimmed.len() > 10 {
        return CheatResult::Cheat {
            message: format!(
                "player name \"{}\" is {} characters long (maximum is 10)",
                trimmed,
                trimmed.len()
            ),
        };
    }
    CheatResult::Allow
}

/// Reject a string name that exceeds the given byte-length limit.
pub fn check_name_length(name: &str, max_len: usize) -> CheatResult {
    let len = name.trim().len();
    if len > max_len {
        CheatResult::Cheat {
            message: format!(
                "name length {} exceeds maximum allowed length of {}",
                len, max_len
            ),
        }
    } else {
        CheatResult::Allow
    }
}

// ── Host & ownership ────────────────────────────────────────────────

/// Reject host-only actions performed by a non-host player.
pub fn check_is_host(player_is_host: bool) -> CheatResult {
    if !player_is_host {
        CheatResult::Cheat {
            message: "host-only operation attempted by a non-host player".into(),
        }
    } else {
        CheatResult::Allow
    }
}

/// Reject actions on an object the player does not own (unless the player
/// is the host, who may act on any object).
pub fn check_ownership(
    object_owner_id: ClientId,
    player_id: ClientId,
    player_is_host: bool,
) -> CheatResult {
    if object_owner_id == player_id || player_is_host {
        CheatResult::Allow
    } else {
        CheatResult::Cheat {
            message: format!(
                "player {} does not own object (owner is {}); host override: {}",
                player_id, object_owner_id, player_is_host
            ),
        }
    }
}

// ── Color checks ────────────────────────────────────────────────────

/// Reject colors that lie outside the valid range 0–17.
pub fn check_color_in_range(color: u8) -> CheatResult {
    if color > 17 {
        CheatResult::Cheat {
            message: format!(
                "color {} is out of valid range 0–17",
                color
            ),
        }
    } else {
        CheatResult::Allow
    }
}

/// Reject a color that is already claimed by another player.
pub fn check_color_used(color: u8, used_colors: &[u8]) -> CheatResult {
    if used_colors.contains(&color) {
        CheatResult::Cheat {
            message: format!(
                "color {} is already in use by another player",
                color
            ),
        }
    } else {
        CheatResult::Allow
    }
}

// ── Game-state gating ───────────────────────────────────────────────

/// Reject RPC calls that are not valid in the current game state.
///
/// * **Lobby RPCs** (`SetName`, `SetColor`, `SetHat`, `SetSkin`, etc.) are
///   only valid when the game has not started.
/// * **In-game RPCs** (`MurderPlayer`, `CompleteTask`, `EnterVent`, etc.)
///   are only valid while the game is running.
///
/// Meeting-related RPCs (`StartMeeting`, `CastVote`, etc.) are valid both
/// while playing AND during a meeting (which occurs during `Started` state).
pub fn check_game_state_rpc(state: GameState, rpc_name: &str) -> CheatResult {
    // RPCs that are only allowed in the lobby (before the game starts).
    let lobby_only_rpcs = [
        "SetName", "CheckName", "SetColor", "CheckColor",
        "SetHat", "SetSkin", "SetPet", "SetVisor", "SetNamePlate",
        "SetHatStr", "SetSkinStr", "SetPetStr", "SetVisorStr", "SetNamePlateStr",
        "SetScanner", "SetLevel",
        "SetRole",
        "SyncSettings",
        "SetStartCounter",
        "StartGame",
        "LobbyTimeExpiring", "ExtendLobbyTimer",
    ];

    // RPCs that require gameplay to be active.
    let playing_only_rpcs = [
        "MurderPlayer", "ReportDeadBody", "CompleteTask",
        "EnterVent", "ExitVent", "BootFromVent",
        "ClimbLadder", "UsePlatform",
        "SnapTo",
        "CloseDoorsOfType", "UpdateSystem",
        "CheckMurder", "CheckProtect",
        "ProtectPlayer",
        "Shapeshift", "CheckShapeshift", "RejectShapeshift",
        "CheckZipline", "UseZipline",
        "TriggerSpores", "CheckSpore",
        "CheckVanish", "StartVanish", "CheckAppear", "StartAppear",
        "Pet", "CancelPet",
    ];

    match state {
        GameState::NotStarted | GameState::Starting => {
            if playing_only_rpcs.contains(&rpc_name) {
                return CheatResult::Cheat {
                    message: format!(
                        "RPC '{}' requires gameplay to be active, but game state is {:?}",
                        rpc_name, state
                    ),
                };
            }
        }
        GameState::Started => {
            if lobby_only_rpcs.contains(&rpc_name) {
                return CheatResult::Cheat {
                    message: format!(
                        "RPC '{}' is a lobby-only RPC, but game state is {:?}",
                        rpc_name, state
                    ),
                };
            }
        }
        GameState::Ended | GameState::Destroyed => {
            return CheatResult::Cheat {
                message: format!(
                    "RPC '{}' is not allowed when game state is {:?}",
                    rpc_name, state
                ),
            };
        }
    }

    CheatResult::Allow
}

// ── Player-state gating ─────────────────────────────────────────────

/// Reject actions that dead players are not allowed to perform.
///
/// Dead players can still chat and vote, but cannot move, kill, complete
/// tasks, or use vents.
pub fn check_player_alive(is_dead: bool, rpc_name: &str) -> CheatResult {
    // RPCs that dead players are still allowed to use.
    let dead_allowed_rpcs = [
        "SendChat", "SendQuickChat", "SendChatNote",
        "CastVote", "ClearVote", "AddVote",
        "VotingComplete",
    ];

    if is_dead && !dead_allowed_rpcs.contains(&rpc_name) {
        CheatResult::Cheat {
            message: format!(
                "dead player attempted RPC '{}' which requires the player to be alive",
                rpc_name
            ),
        }
    } else {
        CheatResult::Allow
    }
}

/// Reject impostor-only RPCs when the sender is a crewmate.
///
/// Impostor-exclusive RPCs include murder, sabotage, and vent usage
/// (unless the player has the Engineer role).
pub fn check_player_impostor(is_impostor: bool, rpc_name: &str) -> CheatResult {
    let impostor_only_rpcs = [
        "MurderPlayer",
        "CheckMurder",
        "CloseDoorsOfType",
        "UpdateSystem",
    ];

    if impostor_only_rpcs.contains(&rpc_name) && !is_impostor {
        CheatResult::Cheat {
            message: format!(
                "non-impostor player attempted impostor-only RPC '{}'",
                rpc_name
            ),
        }
    } else {
        CheatResult::Allow
    }
}

/// Reject an action when the player does not have any of the required roles.
///
/// `player_role` is the role currently assigned to the player.
/// `required_roles` is the set of roles that are allowed to perform the action.
pub fn check_player_has_role(
    player_role: RoleTypes,
    required_roles: &[RoleTypes],
) -> CheatResult {
    if required_roles.iter().any(|r| *r == player_role) {
        CheatResult::Allow
    } else {
        let role_names: Vec<String> = required_roles
            .iter()
            .map(|r| format!("{:?}", r))
            .collect();
        CheatResult::Cheat {
            message: format!(
                "player with role {:?} attempted action that requires one of: {}",
                player_role,
                role_names.join(", ")
            ),
        }
    }
}

// ── Kill validation ─────────────────────────────────────────────────

/// Reject a kill attempt when the distance between killer and victim exceeds
/// the allowed maximum.
///
/// Positions are given as `(x, y)` tuples. Distance is Euclidean.
pub fn check_kill_distance(
    player_pos: (f32, f32),
    target_pos: (f32, f32),
    max_distance: f32,
) -> CheatResult {
    let dx = player_pos.0 - target_pos.0;
    let dy = player_pos.1 - target_pos.1;
    let distance = (dx * dx + dy * dy).sqrt();

    if distance > max_distance {
        CheatResult::Cheat {
            message: format!(
                "kill distance {:.2} exceeds maximum allowed distance of {:.2} \
                 (killer at ({:.2}, {:.2}), victim at ({:.2}, {:.2}))",
                distance, max_distance,
                player_pos.0, player_pos.1,
                target_pos.0, target_pos.1
            ),
        }
    } else {
        CheatResult::Allow
    }
}

/// Reject a kill attempt when the kill cooldown has not yet expired.
///
/// `last_kill_time` and `current_time` should be in the same units
/// (typically milliseconds since game start).
pub fn check_kill_cooldown(
    last_kill_time: u64,
    current_time: u64,
    cooldown_ms: u64,
) -> CheatResult {
    if current_time < last_kill_time {
        // Clock skew or wraparound — allow but log-worthy.
        return CheatResult::Allow;
    }
    let elapsed = current_time - last_kill_time;
    if elapsed < cooldown_ms {
        let remaining = cooldown_ms - elapsed;
        CheatResult::Cheat {
            message: format!(
                "kill cooldown has not expired: {} ms remaining (cooldown is {} ms, \
                 {} ms elapsed since last kill)",
                remaining, cooldown_ms, elapsed
            ),
        }
    } else {
        CheatResult::Allow
    }
}

// ── Vent validation ─────────────────────────────────────────────────

/// Reject vent usage by players who are not allowed to vent.
///
/// * Impostors can always use vents.
/// * Engineers (role) can use vents.
/// * Crewmates without the Engineer role cannot use vents.
///
/// `current_vent_id` is checked for validity — `u32::MAX` is treated as
/// "no vent" and rejected.
pub fn check_vent_usage(
    player_role_is_engineer: bool,
    current_vent_id: u32,
    is_impostor: bool,
) -> CheatResult {
    if current_vent_id == u32::MAX {
        return CheatResult::Cheat {
            message: "attempted to use an invalid vent (vent ID is MAX)".into(),
        };
    }

    if !is_impostor && !player_role_is_engineer {
        CheatResult::Cheat {
            message: format!(
                "non-impostor, non-engineer player attempted to use vent {}",
                current_vent_id
            ),
        }
    } else {
        CheatResult::Allow
    }
}

// ── Task validation ─────────────────────────────────────────────────

/// Reject task completion for a task index that does not exist for the player.
///
/// `player_tasks` is the total number of tasks assigned to the player.
/// Valid task indices are in the range `0..player_tasks`.
pub fn check_task_exists(task_index: u32, player_tasks: u32) -> CheatResult {
    if task_index >= player_tasks {
        CheatResult::Cheat {
            message: format!(
                "task index {} is out of range (player has {} tasks, valid indices are 0..{})",
                task_index, player_tasks, player_tasks.saturating_sub(1)
            ),
        }
    } else {
        CheatResult::Allow
    }
}

// ── Meeting & sabotage gating ───────────────────────────────────────

/// Reject RPCs that are not allowed while a meeting is active.
///
/// During a meeting, only vote-related and chat RPCs are permitted.
pub fn check_meeting_active(is_meeting_active: bool, rpc_name: &str) -> CheatResult {
    // RPCs that are allowed during a meeting.
    let meeting_allowed_rpcs = [
        "CastVote", "ClearVote", "AddVote", "VotingComplete",
        "CloseMeeting", "StartMeeting",
        "SendChat", "SendQuickChat", "SendChatNote",
        "SetInfected", "Exiled",
    ];

    if is_meeting_active && !meeting_allowed_rpcs.contains(&rpc_name) {
        CheatResult::Cheat {
            message: format!(
                "RPC '{}' is not allowed while a meeting is active",
                rpc_name
            ),
        }
    } else {
        CheatResult::Allow
    }
}

/// Reject actions that are not allowed while a meeting is active (simpler form).
///
/// This is a strict gate: if a meeting is active, the action is rejected
/// regardless of what it is. Use this for actions that can NEVER occur during
/// a meeting (e.g., movement, venting).
pub fn check_not_in_meeting(is_meeting_active: bool) -> CheatResult {
    if is_meeting_active {
        CheatResult::Cheat {
            message: "action is not allowed while a meeting is active".into(),
        }
    } else {
        CheatResult::Allow
    }
}

/// Reject actions that are blocked while a sabotage crisis is in progress.
///
/// During an active sabotage (e.g., reactor meltdown, O2 deprivation),
/// certain RPCs or actions may be restricted until the sabotage is resolved.
pub fn check_sabotage_active(is_sabotaged: bool, rpc_name: &str) -> CheatResult {
    // RPCs that remain available during a sabotage crisis.
    let sabotage_allowed_rpcs = [
        "CompleteTask",
        "ReportDeadBody",
        "StartMeeting",
        "SendChat", "SendQuickChat", "SendChatNote",
        "UpdateSystem",  // fixing the sabotage itself
        "CastVote", "ClearVote", "AddVote", "VotingComplete",
        "CloseMeeting",
        "SetInfected", "Exiled",
    ];

    if is_sabotaged && !sabotage_allowed_rpcs.contains(&rpc_name) {
        CheatResult::Cheat {
            message: format!(
                "RPC '{}' is not allowed while a sabotage crisis is active",
                rpc_name
            ),
        }
    } else {
        CheatResult::Allow
    }
}

// ── Vote validation ─────────────────────────────────────────────────

/// Reject a vote when the player has already voted or is dead.
///
/// Dead players cannot cast votes in standard Among Us; ghost votes are
/// tracked separately. Players who have already voted cannot change their
/// vote (the protocol expects a `ClearVote` first if vote-changing is
/// enabled by host options).
pub fn check_vote_already_cast(has_voted: bool, is_dead: bool) -> CheatResult {
    if is_dead {
        return CheatResult::Cheat {
            message: "dead player attempted to cast a vote".into(),
        };
    }
    if has_voted {
        return CheatResult::Cheat {
            message: "player has already cast a vote in this meeting".into(),
        };
    }
    CheatResult::Allow
}

// ── Ability cooldowns ───────────────────────────────────────────────

/// Reject a shapeshift attempt when the cooldown has not yet expired.
///
/// Times should be in the same units (typically milliseconds).
pub fn check_shapeshift_cooldown(
    last_shift_time: u64,
    current_time: u64,
    cooldown_ms: u64,
) -> CheatResult {
    if current_time < last_shift_time {
        return CheatResult::Allow;
    }
    let elapsed = current_time - last_shift_time;
    if elapsed < cooldown_ms {
        let remaining = cooldown_ms - elapsed;
        CheatResult::Cheat {
            message: format!(
                "shapeshift cooldown has not expired: {} ms remaining \
                 (cooldown is {} ms, {} ms elapsed)",
                remaining, cooldown_ms, elapsed
            ),
        }
    } else {
        CheatResult::Allow
    }
}

/// Reject a protect (guardian angel) attempt when the cooldown has not
/// yet expired.
pub fn check_protect_cooldown(
    last_protect_time: u64,
    current_time: u64,
    cooldown_ms: u64,
) -> CheatResult {
    if current_time < last_protect_time {
        return CheatResult::Allow;
    }
    let elapsed = current_time - last_protect_time;
    if elapsed < cooldown_ms {
        let remaining = cooldown_ms - elapsed;
        CheatResult::Cheat {
            message: format!(
                "protect cooldown has not expired: {} ms remaining \
                 (cooldown is {} ms, {} ms elapsed)",
                remaining, cooldown_ms, elapsed
            ),
        }
    } else {
        CheatResult::Allow
    }
}

/// Reject a vanish (phantom/ghost) attempt when the cooldown has not yet
/// expired.
pub fn check_vanish_cooldown(
    last_vanish_time: u64,
    current_time: u64,
    cooldown_ms: u64,
) -> CheatResult {
    if current_time < last_vanish_time {
        return CheatResult::Allow;
    }
    let elapsed = current_time - last_vanish_time;
    if elapsed < cooldown_ms {
        let remaining = cooldown_ms - elapsed;
        CheatResult::Cheat {
            message: format!(
                "vanish cooldown has not expired: {} ms remaining \
                 (cooldown is {} ms, {} ms elapsed)",
                remaining, cooldown_ms, elapsed
            ),
        }
    } else {
        CheatResult::Allow
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use railway_protocol::RoleTypes;

    // ── packet size ─────────────────────────────────────────────

    #[test]
    fn packet_size_within_limit() {
        assert!(check_packet_size(100, 1024).is_allow());
    }

    #[test]
    fn packet_size_exceeds_limit() {
        let r = check_packet_size(2048, 1024);
        assert!(r.is_cheat());
        assert!(r.cheat_message().unwrap().contains("2048"));
    }

    // ── player name ────────────────────────────────────────────

    #[test]
    fn valid_player_name() {
        assert!(check_player_name("Alice").is_allow());
    }

    #[test]
    fn empty_player_name() {
        assert!(check_player_name("").is_cheat());
    }

    #[test]
    fn whitespace_player_name() {
        assert!(check_player_name("   ").is_cheat());
    }

    #[test]
    fn player_name_too_long() {
        let name = "ThisNameIsWayTooLong";
        let r = check_player_name(name);
        assert!(r.is_cheat());
        assert!(r.cheat_message().unwrap().contains("14"));
    }

    #[test]
    fn player_name_exactly_10_chars() {
        assert!(check_player_name("0123456789").is_allow());
    }

    // ── name length ────────────────────────────────────────────

    #[test]
    fn name_length_ok() {
        assert!(check_name_length("hello", 10).is_allow());
    }

    #[test]
    fn name_length_exceeded() {
        assert!(check_name_length("this is too long", 10).is_cheat());
    }

    // ── host check ─────────────────────────────────────────────

    #[test]
    fn host_passes_check() {
        assert!(check_is_host(true).is_allow());
    }

    #[test]
    fn non_host_fails_check() {
        assert!(check_is_host(false).is_cheat());
    }

    // ── ownership ──────────────────────────────────────────────

    #[test]
    fn owner_passes_check() {
        assert!(check_ownership(42, 42, false).is_allow());
    }

    #[test]
    fn host_overrides_ownership() {
        assert!(check_ownership(99, 42, true).is_allow());
    }

    #[test]
    fn non_owner_fails_check() {
        assert!(check_ownership(99, 42, false).is_cheat());
    }

    // ── color range ────────────────────────────────────────────

    #[test]
    fn color_in_range() {
        for c in 0..=17 {
            assert!(check_color_in_range(c).is_allow(), "color {} should be valid", c);
        }
    }

    #[test]
    fn color_out_of_range() {
        assert!(check_color_in_range(18).is_cheat());
        assert!(check_color_in_range(255).is_cheat());
    }

    // ── color used ─────────────────────────────────────────────

    #[test]
    fn color_not_used() {
        assert!(check_color_used(3, &[1, 2, 4]).is_allow());
    }

    #[test]
    fn color_in_use() {
        assert!(check_color_used(2, &[1, 2, 4]).is_cheat());
    }

    #[test]
    fn color_empty_used_list() {
        assert!(check_color_used(0, &[]).is_allow());
    }

    // ── game state rpc ─────────────────────────────────────────

    #[test]
    fn lobby_rpc_in_lobby_is_ok() {
        assert!(check_game_state_rpc(GameState::NotStarted, "SetName").is_allow());
    }

    #[test]
    fn playing_rpc_in_lobby_is_cheat() {
        assert!(check_game_state_rpc(GameState::NotStarted, "MurderPlayer").is_cheat());
    }

    #[test]
    fn lobby_rpc_during_game_is_cheat() {
        assert!(check_game_state_rpc(GameState::Started, "SetColor").is_cheat());
    }

    #[test]
    fn any_rpc_in_ended_is_cheat() {
        assert!(check_game_state_rpc(GameState::Ended, "SendChat").is_cheat());
    }

    #[test]
    fn any_rpc_in_destroyed_is_cheat() {
        assert!(check_game_state_rpc(GameState::Destroyed, "SendChat").is_cheat());
    }

    // ── player alive ───────────────────────────────────────────

    #[test]
    fn alive_player_can_kill() {
        assert!(check_player_alive(false, "MurderPlayer").is_allow());
    }

    #[test]
    fn dead_player_can_chat() {
        assert!(check_player_alive(true, "SendChat").is_allow());
    }

    #[test]
    fn dead_player_cannot_kill() {
        assert!(check_player_alive(true, "MurderPlayer").is_cheat());
    }

    #[test]
    fn dead_player_cannot_vent() {
        assert!(check_player_alive(true, "EnterVent").is_cheat());
    }

    // ── player impostor ────────────────────────────────────────

    #[test]
    fn impostor_can_murder() {
        assert!(check_player_impostor(true, "MurderPlayer").is_allow());
    }

    #[test]
    fn crewmate_cannot_murder() {
        assert!(check_player_impostor(false, "MurderPlayer").is_cheat());
    }

    #[test]
    fn crewmate_can_do_non_impostor_rpc() {
        assert!(check_player_impostor(false, "CompleteTask").is_allow());
    }

    // ── kill distance ──────────────────────────────────────────

    #[test]
    fn kill_within_range() {
        let r = check_kill_distance((0.0, 0.0), (1.0, 1.0), 2.0);
        // sqrt(2) ≈ 1.414 < 2.0
        assert!(r.is_allow());
    }

    #[test]
    fn kill_out_of_range() {
        let r = check_kill_distance((0.0, 0.0), (10.0, 0.0), 5.0);
        // distance = 10 > 5
        assert!(r.is_cheat());
    }

    // ── kill cooldown ──────────────────────────────────────────

    #[test]
    fn kill_cooldown_expired() {
        assert!(check_kill_cooldown(0, 30_000, 25_000).is_allow());
    }

    #[test]
    fn kill_cooldown_not_expired() {
        assert!(check_kill_cooldown(0, 5_000, 25_000).is_cheat());
    }

    #[test]
    fn kill_cooldown_exact_boundary() {
        assert!(check_kill_cooldown(0, 25_000, 25_000).is_allow());
    }

    // ── vent usage ─────────────────────────────────────────────

    #[test]
    fn impostor_can_vent() {
        assert!(check_vent_usage(false, 1, true).is_allow());
    }

    #[test]
    fn engineer_can_vent() {
        assert!(check_vent_usage(true, 1, false).is_allow());
    }

    #[test]
    fn crewmate_cannot_vent() {
        assert!(check_vent_usage(false, 1, false).is_cheat());
    }

    #[test]
    fn invalid_vent_id() {
        assert!(check_vent_usage(false, u32::MAX, true).is_cheat());
    }

    // ── task exists ────────────────────────────────────────────

    #[test]
    fn task_index_valid() {
        assert!(check_task_exists(0, 5).is_allow());
        assert!(check_task_exists(4, 5).is_allow());
    }

    #[test]
    fn task_index_invalid() {
        assert!(check_task_exists(5, 5).is_cheat());
        assert!(check_task_exists(10, 3).is_cheat());
    }

    // ── meeting active ─────────────────────────────────────────

    #[test]
    fn vote_allowed_during_meeting() {
        assert!(check_meeting_active(true, "CastVote").is_allow());
    }

    #[test]
    fn kill_not_allowed_during_meeting() {
        assert!(check_meeting_active(true, "MurderPlayer").is_cheat());
    }

    #[test]
    fn anything_allowed_when_no_meeting() {
        assert!(check_meeting_active(false, "MurderPlayer").is_allow());
    }

    // ── not in meeting (strict) ────────────────────────────────

    #[test]
    fn strict_not_in_meeting_blocks_all() {
        assert!(check_not_in_meeting(true).is_cheat());
    }

    #[test]
    fn strict_not_in_meeting_allows_when_no_meeting() {
        assert!(check_not_in_meeting(false).is_allow());
    }

    // ── sabotage active ────────────────────────────────────────

    #[test]
    fn fix_sabotage_allowed_during_sabotage() {
        assert!(check_sabotage_active(true, "UpdateSystem").is_allow());
    }

    #[test]
    fn vent_not_allowed_during_sabotage() {
        // EnterVent is not in the sabotage-allowed list.
        assert!(check_sabotage_active(true, "EnterVent").is_cheat());
    }

    #[test]
    fn anything_allowed_when_no_sabotage() {
        assert!(check_sabotage_active(false, "EnterVent").is_allow());
    }

    // ── vote already cast ──────────────────────────────────────

    #[test]
    fn first_vote_allowed() {
        assert!(check_vote_already_cast(false, false).is_allow());
    }

    #[test]
    fn duplicate_vote_rejected() {
        assert!(check_vote_already_cast(true, false).is_cheat());
    }

    #[test]
    fn dead_player_vote_rejected() {
        assert!(check_vote_already_cast(false, true).is_cheat());
    }

    // ── shapeshift cooldown ────────────────────────────────────

    #[test]
    fn shapeshift_cooldown_expired() {
        assert!(check_shapeshift_cooldown(0, 15_000, 10_000).is_allow());
    }

    #[test]
    fn shapeshift_cooldown_not_expired() {
        assert!(check_shapeshift_cooldown(0, 3_000, 10_000).is_cheat());
    }

    // ── protect cooldown ───────────────────────────────────────

    #[test]
    fn protect_cooldown_expired() {
        assert!(check_protect_cooldown(0, 40_000, 35_000).is_allow());
    }

    #[test]
    fn protect_cooldown_not_expired() {
        assert!(check_protect_cooldown(0, 5_000, 35_000).is_cheat());
    }

    // ── vanish cooldown ────────────────────────────────────────

    #[test]
    fn vanish_cooldown_expired() {
        assert!(check_vanish_cooldown(0, 20_000, 15_000).is_allow());
    }

    #[test]
    fn vanish_cooldown_not_expired() {
        assert!(check_vanish_cooldown(0, 1_000, 15_000).is_cheat());
    }

    // ── player has role ────────────────────────────────────────

    #[test]
    fn player_has_required_role() {
        use RoleTypes::*;
        assert!(check_player_has_role(Impostor, &[Impostor, Shapeshifter]).is_allow());
    }

    #[test]
    fn player_lacks_required_role() {
        use RoleTypes::*;
        assert!(check_player_has_role(Crewmate, &[Impostor, Shapeshifter]).is_cheat());
    }

    #[test]
    fn player_has_role_in_singleton_list() {
        use RoleTypes::*;
        assert!(check_player_has_role(Scientist, &[Scientist]).is_allow());
    }

    // ── validate_all ───────────────────────────────────────────

    #[test]
    fn validate_all_ok() {
        use super::super::validate_all;
        let results = [
            check_player_name("Bob"),
            check_color_in_range(5),
            check_is_host(true),
        ];
        assert!(validate_all(&results).is_ok());
    }

    #[test]
    fn validate_all_first_cheat_stops() {
        use super::super::validate_all;
        let results = [
            check_player_name(""),          // fails
            check_color_in_range(255),      // also fails, but shouldn't be reached
        ];
        let err = validate_all(&results).unwrap_err();
        assert!(matches!(err, crate::error::GameError::CheatDetected { .. }));
        assert!(err.to_string().contains("empty"));
    }
}
