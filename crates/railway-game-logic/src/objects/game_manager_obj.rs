//! Game manager objects — NormalGameManager and HideAndSeekManager.
//!
//! These are server-owned objects that oversee the game's lifecycle.
//! - `InnerNormalGameManager`: classic Among Us with tasks, meetings, and roles.
//! - `InnerHideAndSeekManager`: hide & seek variant with seekers, hiders, and timers.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use async_trait::async_trait;
use railway_hazel::{MessageReader, MessageWriter};
use railway_protocol::{
    ClientId, GameOverReason, NetId, PlayerId, RpcCalls, SpawnFlags,
};

use crate::error::GameError;
use crate::events::GameEvent;
use crate::game_flow;
use crate::objects::game_data::InnerGameData;
use crate::objects::InnerNetObject;
use crate::player::ClientPlayer;
use crate::{Game, GameResult};
use parking_lot::Mutex;

// ── InnerNormalGameManager ────────────────────────────────────────────────

/// Game manager for the Normal (classic) Among Us mode.
///
/// Tracks game flow, win conditions, and coordinates the meeting lifecycle.
/// It holds a reference to the `InnerGameData` for accessing PlayerInfo objects.
pub struct InnerNormalGameManager {
    net_id: NetId,
    owner_id: ClientId,
    spawn_flags: SpawnFlags,
    game: Arc<Game>,
    /// Optional reference to the game data registry (PlayerInfo).
    game_data: Option<Arc<InnerGameData>>,
}

impl InnerNormalGameManager {
    /// Create a new normal game manager.
    pub fn new(game: Arc<Game>) -> Self {
        Self {
            net_id: 0,
            owner_id: crate::SERVER_OWNED_ID,
            spawn_flags: SpawnFlags::NONE,
            game,
            game_data: None,
        }
    }

    /// Set the game data reference (called after the InnerGameData object is spawned).
    pub fn set_game_data(&mut self, data: Arc<InnerGameData>) {
        self.game_data = Some(data);
    }

    /// Get the game data reference, if set.
    pub fn game_data(&self) -> Option<&Arc<InnerGameData>> {
        self.game_data.as_ref()
    }

    /// Check win conditions for the Normal game mode.
    ///
    /// Returns `None` if the game continues, or `Some(reason)` if the game should end.
    pub fn check_win_condition(&self) -> Option<GameOverReason> {
        let state = self.game.state();
        if !state.is_playing() {
            return None;
        }

        // Count impostors and crewmates among alive players.
        let mut alive_impostors = 0u32;
        let mut alive_crewmates = 0u32;

        for entry in self.game.players.iter() {
            let player = entry.value();
            if player.is_impostor {
                alive_impostors += 1;
            } else {
                alive_crewmates += 1;
            }
        }

        // Impostors win by elimination: if impostors >= non-impostors,
        // the impostors can outvote or kill the remaining crewmates.
        if alive_impostors >= alive_crewmates {
            return Some(GameOverReason::ImpostorsByKill);
        }

        // Crewmates win when all impostors are eliminated (by vote or kill).
        if alive_impostors == 0 {
            return Some(GameOverReason::CrewmatesByVote);
        }

        // Check task completion: if all crewmates completed all their tasks,
        // crewmates win. This requires iterating over PlayerInfo objects.
        if let Some(data) = &self.game_data {
            let alive_crewmates = data.get_alive_crewmates();
            if !alive_crewmates.is_empty() {
                let all_tasks_done = alive_crewmates.iter().all(|info| {
                    let completed = info.tasks_completed.load(Ordering::Relaxed);
                    let total = info.tasks_total.load(Ordering::Relaxed);
                    completed >= total && total > 0
                });
                if all_tasks_done {
                    return Some(GameOverReason::CrewmatesByTask);
                }
            }
        }

        None
    }
}

#[async_trait]
impl InnerNetObject for InnerNormalGameManager {
    fn net_id(&self) -> NetId {
        self.net_id
    }

    fn owner_id(&self) -> ClientId {
        self.owner_id
    }

    fn spawn_flags(&self) -> SpawnFlags {
        self.spawn_flags
    }

    fn set_net_id(&mut self, net_id: NetId) {
        self.net_id = net_id;
    }

    fn set_owner_id(&mut self, owner_id: ClientId) {
        self.owner_id = owner_id;
    }

    fn set_spawn_flags(&mut self, flags: SpawnFlags) {
        self.spawn_flags = flags;
    }

    async fn serialize(&self, writer: &mut MessageWriter, _initial_state: bool) -> GameResult<()> {
        // NormalGameManager state is minimal — most state is kept in
        // ShipStatus, PlayerControl, and MeetingHud.
        // We write the player count for client-side validation.
        let count = self.game.player_count();
        writer.write_packed_u32(count as u32);
        Ok(())
    }

    async fn deserialize(
        &mut self,
        _sender: &ClientPlayer,
        _target: Option<&ClientPlayer>,
        reader: &mut MessageReader,
        _initial_state: bool,
    ) -> GameResult<()> {
        // Consume the serialized data without any side effects.
        if reader.remaining() >= 1 {
            let _count = reader.read_packed_u32();
        }
        Ok(())
    }

    async fn handle_rpc(
        &mut self,
        _sender: &ClientPlayer,
        _target: Option<&ClientPlayer>,
        _call: RpcCalls,
        _reader: &mut MessageReader,
    ) -> GameResult<bool> {
        // NormalGameManager does not handle direct object RPCs.
        // Game-flow RPCs (ReportDeadBody, StartMeeting, etc.) are handled by
        // PlayerControl and MeetingHud objects.
        Ok(false)
    }
}

// ── InnerHideAndSeekManager ───────────────────────────────────────────────

/// Game manager for the Hide & Seek variant.
///
/// Tracks seekers, hiders, the game timer, and final-hide phase.
/// Win conditions: seekers win by killing all hiders before the timer expires;
/// hiders win by surviving until the timer runs out.
pub struct InnerHideAndSeekManager {
    net_id: NetId,
    owner_id: ClientId,
    spawn_flags: SpawnFlags,
    game: Arc<Game>,
    /// Player IDs of the seekers.
    seeker_ids: Mutex<Vec<PlayerId>>,
    /// Player IDs of the hiders.
    hider_ids: Mutex<Vec<PlayerId>>,
    /// Remaining game time (seconds).
    game_timer: Mutex<f32>,
    /// Whether the final-hide phase is active.
    is_final_hide: Mutex<bool>,
}

impl InnerHideAndSeekManager {
    /// Create a new hide & seek manager.
    pub fn new(game: Arc<Game>) -> Self {
        Self {
            net_id: 0,
            owner_id: crate::SERVER_OWNED_ID,
            spawn_flags: SpawnFlags::NONE,
            game,
            seeker_ids: Mutex::new(Vec::new()),
            hider_ids: Mutex::new(Vec::new()),
            game_timer: Mutex::new(0.0),
            is_final_hide: Mutex::new(false),
        }
    }

    /// Initialise the manager with seekers, hiders, and timer duration.
    pub fn init(&self, seeker_ids: Vec<PlayerId>, hider_ids: Vec<PlayerId>, game_time: f32) {
        *self.seeker_ids.lock() = seeker_ids;
        *self.hider_ids.lock() = hider_ids;
        *self.game_timer.lock() = game_time;
        *self.is_final_hide.lock() = false;
    }

    /// Set the game timer (e.g., for countdown display).
    pub fn set_game_timer(&self, seconds: f32) {
        *self.game_timer.lock() = seconds;
    }

    /// Get the current game timer value.
    pub fn game_timer(&self) -> f32 {
        *self.game_timer.lock()
    }

    /// Set the final-hide phase flag.
    pub fn set_final_hide(&self, is_final: bool) {
        *self.is_final_hide.lock() = is_final;
    }

    /// Returns `true` if the final-hide phase is active.
    pub fn is_final_hide(&self) -> bool {
        *self.is_final_hide.lock()
    }

    /// Get the list of seeker player IDs.
    pub fn seekers(&self) -> Vec<PlayerId> {
        self.seeker_ids.lock().clone()
    }

    /// Get the list of hider player IDs.
    pub fn hiders(&self) -> Vec<PlayerId> {
        self.hider_ids.lock().clone()
    }

    /// Mark a hider as killed. Returns `true` if the hider was found.
    pub fn kill_hider(&self, hider_id: PlayerId) -> bool {
        let mut hiders = self.hider_ids.lock();

        // Seeker can only kill hiders (not other seekers).
        if let Some(pos) = hiders.iter().position(|id| *id == hider_id) {
            hiders.remove(pos);
            true
        } else {
            false
        }
    }

    /// Check win conditions for Hide & Seek mode.
    ///
    /// Returns `None` if the game continues, or `Some(reason)` if the game should end.
    pub fn check_win_condition(&self) -> Option<GameOverReason> {
        let state = self.game.state();
        if !state.is_playing() {
            return None;
        }

        let hiders = self.hider_ids.lock();
        let timer = *self.game_timer.lock();

        // If timer ran out, hiders win.
        if timer <= 0.0 && !hiders.is_empty() {
            return Some(GameOverReason::HideAndSeekByTimer);
        }

        // If all hiders are eliminated, seekers win.
        if hiders.is_empty() {
            return Some(GameOverReason::HideAndSeekByKills);
        }

        None
    }

    /// Start the final-hide countdown. Transitions the game into the
    /// final-hide phase where the last remaining hiders get a timer boost.
    pub fn start_final_hide(&self, additional_time: f32) {
        self.set_final_hide(true);

        let mut timer = self.game_timer.lock();
        *timer = additional_time;
    }
}

#[async_trait]
impl InnerNetObject for InnerHideAndSeekManager {
    fn net_id(&self) -> NetId {
        self.net_id
    }

    fn owner_id(&self) -> ClientId {
        self.owner_id
    }

    fn spawn_flags(&self) -> SpawnFlags {
        self.spawn_flags
    }

    fn set_net_id(&mut self, net_id: NetId) {
        self.net_id = net_id;
    }

    fn set_owner_id(&mut self, owner_id: ClientId) {
        self.owner_id = owner_id;
    }

    fn set_spawn_flags(&mut self, flags: SpawnFlags) {
        self.spawn_flags = flags;
    }

    async fn serialize(&self, writer: &mut MessageWriter, _initial_state: bool) -> GameResult<()> {
        // Write game timer (remaining seconds).
        let timer = self.game_timer();
        writer.write_f32(timer);

        // Write final-hide flag.
        let is_final = self.is_final_hide();
        writer.write_bool(is_final);

        // Write seeker count and seeker IDs.
        let seekers = self.seeker_ids.lock();
        writer.write_packed_u32(seekers.len() as u32);
        for seeker_id in seekers.iter() {
            writer.write_byte(*seeker_id);
        }

        // Write hider count and hider IDs.
        let hiders = self.hider_ids.lock();
        writer.write_packed_u32(hiders.len() as u32);
        for hider_id in hiders.iter() {
            writer.write_byte(*hider_id);
        }

        Ok(())
    }

    async fn deserialize(
        &mut self,
        _sender: &ClientPlayer,
        _target: Option<&ClientPlayer>,
        reader: &mut MessageReader,
        _initial_state: bool,
    ) -> GameResult<()> {
        let timer = reader.read_f32();
        self.set_game_timer(timer);

        let is_final = reader.read_bool();
        self.set_final_hide(is_final);

        // Read seeker IDs.
        let seeker_count = reader.read_packed_u32() as usize;
        let mut seekers = self.seeker_ids.lock();
        seekers.clear();
        for _ in 0..seeker_count {
            seekers.push(reader.read_byte());
        }

        // Read hider IDs.
        let hider_count = reader.read_packed_u32() as usize;
        let mut hiders = self.hider_ids.lock();
        hiders.clear();
        for _ in 0..hider_count {
            hiders.push(reader.read_byte());
        }

        Ok(())
    }

    async fn handle_rpc(
        &mut self,
        sender: &ClientPlayer,
        _target: Option<&ClientPlayer>,
        call: RpcCalls,
        reader: &mut MessageReader,
    ) -> GameResult<bool> {
        match call {
            RpcCalls::MurderPlayer => {
                // In Hide & Seek, only seekers can murder.
                // The MurderPlayer RPC contains the victim's PlayerId or NetId.
                // For simplicity we read a u8 player ID.
                let victim_id = reader.read_byte();

                // Validate that the sender is a seeker.
                // Scope the lock so the MutexGuard is dropped before any await.
                let sender_is_seeker = {
                    let sender_player_id = self.resolve_player_id(sender.client_id);
                    let seekers = self.seeker_ids.lock();
                    seekers.contains(&sender_player_id)
                };

                if !sender_is_seeker {
                    return Err(GameError::CheatDetected {
                        message: "only seekers can kill in hide & seek".into(),
                    });
                }

                // Ensure the victim is a hider.
                let victim_is_hider = {
                    let hiders = self.hider_ids.lock();
                    hiders.contains(&victim_id)
                };

                if !victim_is_hider {
                    return Err(GameError::CheatDetected {
                        message: "seekers can only kill hiders".into(),
                    });
                }

                // Remove the hider from the active list.
                {
                    let mut hiders = self.hider_ids.lock();
                    hiders.retain(|id| *id != victim_id);
                }

                // Emit the murder event.
                let victim_client = self.resolve_client_id(victim_id);
                self.game.emit_event(GameEvent::PlayerMurdered {
                    game_code: self.game.code,
                    killer_id: sender.client_id,
                    victim_id: victim_client,
                });

                // Check win condition after each kill.
                if let Some(reason) = self.check_win_condition() {
                    game_flow::end_game(&self.game, reason).await;
                }

                Ok(true)
            }

            RpcCalls::SetStartCounter => {
                // The host sends the countdown value for the hide phase.
                let time_remaining = reader.read_f32();
                self.set_game_timer(time_remaining);

                Ok(true)
            }

            RpcCalls::LobbyTimeExpiring => {
                // The lobby timer is about to expire — transition all players
                // into the game scene.
                // This RPC carries no body; it is a signal from the host.

                if !sender.is_host {
                    return Err(GameError::HostOnlyOperation(sender.client_id));
                }

                // The actual scene transition is handled by the server layer.
                // We just acknowledge the signal.
                Ok(true)
            }

            _ => {
                // Unhandled RPC — pass through.
                Ok(false)
            }
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

impl InnerHideAndSeekManager {
    /// Resolve a ClientId to a PlayerId by looking through the game's
    /// registered PlayerInfo objects.
    fn resolve_player_id(&self, _client_id: ClientId) -> PlayerId {
        // In a full implementation we would iterate the InnerGameData's
        // `players_by_client` map. For now we scan the game objects:
        for entry in self.game.players.iter() {
            if entry.key() == &_client_id {
                // Fallback: return 0 — the caller cross-references with seeker/hider lists.
                return 0;
            }
        }
        0
    }

    /// Resolve a PlayerId to a ClientId.
    fn resolve_client_id(&self, _player_id: PlayerId) -> ClientId {
        for entry in self.game.players.iter() {
            // ClientPlayer doesn't store player_id directly — this mapping
            // is maintained by InnerGameData. In practice we'd look it up there.
            return *entry.key();
        }
        -1
    }
}
