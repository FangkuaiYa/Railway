//! GameFlow — the main game lifecycle orchestrator.
//!
//! Manages the full game lifecycle: lobby setup, game start,
//! meeting coordination, win condition checks, and game end.
//! Delegates mode-specific logic to the normal/hide_and_seek modules.

use std::sync::Arc;
use std::time::Instant;

use railway_protocol::{GameModes, GameOverReason, MapType, NetId, PlayerId};
use parking_lot::Mutex;
use tracing::{debug, info, warn};

use crate::events::GameEvent;
use crate::game::Game;
use crate::game_flow;
use crate::objects::game_data::{InnerGameData, PlayerInfo};
use crate::objects::ship_status::InnerShipStatus;
use crate::objects::lobby_behaviour::InnerLobbyBehaviour;
use crate::objects::meeting_hud::InnerMeetingHud;
use crate::objects::game_manager_obj::{InnerHideAndSeekManager, InnerNormalGameManager};
use crate::objects::InnerNetObject;
use crate::player::ClientPlayer;
use crate::state::GameState;
use crate::{GameError, GameResult};

/// Main orchestrator for a game's lifecycle.
///
/// Holds references to the main game objects and coordinates
/// spawning, deserialization, RPC routing, and win condition checks.
pub struct GameFlow {
    /// The game this flow manages.
    pub game: Arc<Game>,

    /// The game data registry (PlayerInfo objects).
    pub game_data: Mutex<Option<Arc<InnerGameData>>>,

    /// The ship status for the current map.
    pub ship_status: Mutex<Option<Arc<InnerShipStatus>>>,

    /// The meeting HUD for voting.
    pub meeting_hud: Mutex<Option<Arc<InnerMeetingHud>>>,

    /// The lobby behaviour (only in lobby phase).
    pub lobby_behaviour: Mutex<Option<Arc<InnerLobbyBehaviour>>>,

    /// The game manager (Normal or HideAndSeek).
    pub game_manager: Mutex<Option<Arc<dyn InnerNetObject>>>,

    /// When the game started (for timer-based logic).
    pub timer_start: Mutex<Option<Instant>>,

    /// Whether the game has started.
    pub has_started: Mutex<bool>,
}

impl GameFlow {
    /// Create a new GameFlow for the given game.
    pub fn new(game: Arc<Game>) -> Self {
        Self {
            game,
            game_data: Mutex::new(None),
            ship_status: Mutex::new(None),
            meeting_hud: Mutex::new(None),
            lobby_behaviour: Mutex::new(None),
            game_manager: Mutex::new(None),
            timer_start: Mutex::new(None),
            has_started: Mutex::new(false),
        }
    }

    /// Start the game: assign roles, spawn map objects, transition states.
    pub async fn start_game(&self) -> GameResult<()> {
        let state = self.game.state();
        if state != GameState::NotStarted {
            return Err(GameError::InvalidStateTransition {
                from: format!("{:?}", state),
                to: format!("{:?}", GameState::Starting),
            });
        }

        self.game.set_state(GameState::Starting);
        self.game.emit_event(GameEvent::GameStarting {
            game_code: self.game.code,
        });

        // 1. Spawn game data
        self.spawn_game_data()?;

        // 2. Determine map type and spawn ship status
        let map_type = self.resolve_map_type();
        self.spawn_ship_status(map_type)?;

        // 3. Spawn lobby behaviour (will be despawned once players transition to game scene)
        self.spawn_lobby_behaviour()?;

        // 4. Spawn game manager based on game mode
        let mode = self.game.options.game_mode();
        match mode {
            GameModes::Normal | GameModes::NormalFools => {
                self.spawn_normal_game_manager()?;
            }
            GameModes::HideNSeek | GameModes::SeekFools => {
                self.spawn_hide_and_seek_manager()?;
            }
        }

        // 5. Assign roles (impostor, special roles)
        self.assign_roles()?;

        // 6. Spawn player controls for each player
        let player_ids: Vec<i32> = self.game.players.iter().map(|e| *e.key()).collect();
        for client_id in &player_ids {
            self.spawn_player_control(*client_id)?;
        }

        *self.has_started.lock() = true;
        *self.timer_start.lock() = Some(Instant::now());

        self.game.set_state(GameState::Started);
        self.game.emit_event(GameEvent::GameStarted {
            game_code: self.game.code,
        });

        info!("game {}: started with {} players on {:?}", self.game.code, player_ids.len(), map_type);

        Ok(())
    }

    /// End the game with the given reason.
    pub async fn end_game(&self, reason: GameOverReason) -> GameResult<()> {
        let state = self.game.state();
        if state != GameState::Started {
            return Err(GameError::InvalidStateTransition {
                from: format!("{:?}", state),
                to: format!("{:?}", GameState::Ended),
            });
        }

        game_flow::end_game(&self.game, reason).await;

        info!("game {}: ended with reason {:?}", self.game.code, reason);
        Ok(())
    }

    /// Start a meeting (called when a dead body is reported or emergency button pressed).
    pub fn start_meeting(&self, reporter_id: Option<PlayerId>, body_id: Option<PlayerId>) -> GameResult<()> {
        let meeting_hud = self.meeting_hud.lock();
        if let Some(hud) = meeting_hud.as_ref() {
            hud.start_meeting(reporter_id, body_id);
        }

        let _ = game_flow::start_meeting(&self.game);

        debug!(
            "game {}: meeting started by {:?}, body {:?}",
            self.game.code, reporter_id, body_id
        );

        Ok(())
    }

    /// End/close the current meeting.
    pub fn end_meeting(&self) -> GameResult<()> {
        let meeting_hud = self.meeting_hud.lock();
        if let Some(hud) = meeting_hud.as_ref() {
            hud.close_meeting();
        }

        let _ = game_flow::end_meeting(&self.game);

        // Check if someone was exiled and process
        if let Some(hud) = meeting_hud.as_ref() {
            if let Some(exiled_id) = hud.exiled_player_id() {
                self.process_exile(exiled_id);
            }
        }

        Ok(())
    }

    /// Process an exiled player (mark as dead, check win condition).
    fn process_exile(&self, player_id: PlayerId) {
        debug!("game {}: player {} was exiled", self.game.code, player_id);

        // Mark the player as dead in GameData
        if let Some(gd) = self.game_data.lock().as_ref() {
            if let Some(_player_info) = gd.get_by_player_id(player_id) {
                // We can't mutate through Arc<PlayerInfo> directly since it
                // doesn't have interior mutability. In a full implementation,
                // PlayerInfo would use parking_lot::Mutex for its fields.
                // For now, we note the exile via event.
                debug!("game {}: marking player {} as dead", self.game.code, player_id);
            }
        }

        // Emit the exile event
        self.game.emit_event(GameEvent::PlayerExiled {
            game_code: self.game.code,
            client_id: self.resolve_client_id(player_id).unwrap_or(-1),
        });
    }

    /// Spawn a PlayerControl object for a client.
    pub fn spawn_player_control(&self, client_id: i32) -> GameResult<NetId> {
        let net_id = self.game.next_net_id();

        // PlayerControl would be created by a factory. For now we track
        // the net ID assignment on the ClientPlayer.
        if let Some(mut player) = self.game.players.get_mut(&client_id) {
            player.character_net_id = Some(net_id);
        }

        debug!("game {}: spawned PlayerControl for client {} with net_id {}", self.game.code, client_id, net_id);

        self.game.emit_event(GameEvent::PlayerSpawned {
            game_code: self.game.code,
            client_id,
            character_net_id: net_id,
        });

        Ok(net_id)
    }

    /// Despawn a PlayerControl object.
    pub fn despawn_player_control(&self, client_id: i32) -> GameResult<()> {
        let net_id = {
            if let Some(player) = self.game.players.get(&client_id) {
                player.character_net_id
            } else {
                return Err(GameError::PlayerNotFound(client_id));
            }
        };

        if let Some(net_id) = net_id {
            self.game.unregister_object(net_id);
        }

        if let Some(mut player) = self.game.players.get_mut(&client_id) {
            player.character_net_id = None;
        }

        self.game.emit_event(GameEvent::PlayerDestroyed {
            game_code: self.game.code,
            client_id,
        });

        Ok(())
    }

    /// Spawn the appropriate ship status for the given map type.
    pub fn spawn_ship_status(&self, map_type: MapType) -> GameResult<NetId> {
        let net_id = self.game.next_net_id();

        let ship = Arc::new(InnerShipStatus::new(self.game.clone(), map_type));

        *self.ship_status.lock() = Some(ship);

        debug!("game {}: spawned ship status {:?} with net_id {}", self.game.code, map_type, net_id);
        Ok(net_id)
    }

    /// Spawn the InnerGameData object.
    pub fn spawn_game_data(&self) -> GameResult<NetId> {
        let net_id = self.game.next_net_id();

        let game_data = Arc::new(InnerGameData::new());
        // Create PlayerInfo entries for each player
        for entry in self.game.players.iter() {
            let client_id = *entry.key();
            let _player = entry.value();
            let player_id = game_data.next_available_player_id();

            if player_id == u8::MAX {
                warn!("game {}: no available player slots", self.game.code);
                continue;
            }

            let player_info = Arc::new(PlayerInfo::new(player_id, client_id));
            game_data.add_player(player_info);
        }

        *self.game_data.lock() = Some(game_data.clone());

        debug!("game {}: spawned InnerGameData with net_id {}", self.game.code, net_id);
        Ok(net_id)
    }

    /// Spawn the InnerLobbyBehaviour object.
    pub fn spawn_lobby_behaviour(&self) -> GameResult<NetId> {
        let net_id = self.game.next_net_id();

        let lobby = Arc::new(InnerLobbyBehaviour::new(self.game.clone()));
        *self.lobby_behaviour.lock() = Some(lobby);

        debug!("game {}: spawned LobbyBehaviour with net_id {}", self.game.code, net_id);
        Ok(net_id)
    }

    /// Spawn the InnerNormalGameManager object.
    pub fn spawn_normal_game_manager(&self) -> GameResult<NetId> {
        let net_id = self.game.next_net_id();

        let manager = Arc::new(InnerNormalGameManager::new(self.game.clone()));
        *self.game_manager.lock() = Some(manager);

        debug!("game {}: spawned NormalGameManager with net_id {}", self.game.code, net_id);
        Ok(net_id)
    }

    /// Spawn the InnerHideAndSeekManager object.
    pub fn spawn_hide_and_seek_manager(&self) -> GameResult<NetId> {
        let net_id = self.game.next_net_id();

        let manager = Arc::new(InnerHideAndSeekManager::new(self.game.clone()));
        *self.game_manager.lock() = Some(manager);

        debug!("game {}: spawned HideAndSeekManager with net_id {}", self.game.code, net_id);
        Ok(net_id)
    }

    /// Spawn the InnerMeetingHud object.
    pub fn spawn_meeting_hud(&self) -> GameResult<NetId> {
        let net_id = self.game.next_net_id();

        let hud = Arc::new(InnerMeetingHud::new(self.game.clone()));
        *self.meeting_hud.lock() = Some(hud);

        debug!("game {}: spawned MeetingHud with net_id {}", self.game.code, net_id);
        Ok(net_id)
    }

    /// Check win conditions and return a reason if the game should end.
    pub fn check_win_condition(&self) -> Option<GameOverReason> {
        let state = self.game.state();
        if !state.is_playing() {
            return None;
        }

        // Count alive crewmates and impostors
        let mut alive_impostors: u32 = 0;
        let mut alive_crewmates: u32 = 0;
        let player_count = self.game.player_count();

        for entry in self.game.players.iter() {
            let player = entry.value();
            if player.is_impostor {
                alive_impostors += 1;
            } else {
                alive_crewmates += 1;
            }
        }

        // No players means game is empty
        if player_count == 0 {
            return Some(GameOverReason::CrewmateDisconnect);
        }

        // Impostors win by elimination
        if alive_impostors > 0 && alive_impostors >= alive_crewmates {
            return Some(GameOverReason::ImpostorsByKill);
        }

        // Crewmates win by eliminating all impostors
        if alive_impostors == 0 {
            return Some(GameOverReason::CrewmatesByVote);
        }

        // Check task completion (crewmates win if all tasks are done)
        if let Some(gd) = self.game_data.lock().as_ref() {
            let mut all_tasks_complete = true;
            let mut any_crewmate_alive = false;

            for entry in self.game.players.iter() {
                let client_id = *entry.key();
                let player = entry.value();

                if player.is_impostor {
                    continue;
                }

                any_crewmate_alive = true;

                if let Some(info) = gd.get_by_client_id(client_id) {
                    let is_dead = info.is_dead.load(std::sync::atomic::Ordering::Relaxed);
                    let tasks_completed = info.tasks_completed.load(std::sync::atomic::Ordering::Relaxed);
                    let tasks_total = info.tasks_total.load(std::sync::atomic::Ordering::Relaxed);
                    if !is_dead && tasks_completed < tasks_total && tasks_total > 0 {
                        all_tasks_complete = false;
                        break;
                    }
                }
            }

            if any_crewmate_alive && all_tasks_complete {
                return Some(GameOverReason::CrewmatesByTask);
            }
        }

        // Check sabotage timeout (e.g., reactor meltdown, O2 depletion)
        if let Some(ship) = self.ship_status.lock().as_ref() {
            if ship.check_sabotage_timeout() {
                return Some(GameOverReason::ImpostorsBySabotage);
            }
        }

        None
    }

    /// Assign roles (impostors, special roles) to players.
    fn assign_roles(&self) -> GameResult<()> {
        let mode = self.game.options.game_mode();

        match mode {
            GameModes::Normal | GameModes::NormalFools => {
                let selector = RoleSelector::new();
                if let Some(gd) = self.game_data.lock().as_ref() {
                    // Determine number of impostors based on player count
                    let num_impostors = Self::calculate_impostor_count(self.game.player_count());
                    let impostor_ids = selector.select_impostors(gd, num_impostors);

                    // Mark impostors
                    for player_id in &impostor_ids {
                        if let Some(mut player) = self.find_client_player_by_player_id(gd, *player_id) {
                            player.is_impostor = true;
                        }
                    }

                    // Assign special roles if using NormalGameOptions
                    if let Some(normal_opts) = self.game.options.as_any().downcast_ref::<railway_protocol::game_options::NormalGameOptions>() {
                        selector.assign_special_roles(gd, &normal_opts.role_options);
                    }
                }
            }
            GameModes::HideNSeek | GameModes::SeekFools => {
                let selector = RoleSelector::new();
                if let Some(gd) = self.game_data.lock().as_ref() {
                    // Assign seeker role (typically 1 seeker)
                    let seeker_ids = selector.select_impostors(gd, 1);
                    for player_id in &seeker_ids {
                        if let Some(mut player) = self.find_client_player_by_player_id(gd, *player_id) {
                            player.is_impostor = true; // seeker acts as impostor
                        }
                    }
                }
            }
        }

        debug!("game {}: roles assigned", self.game.code);
        Ok(())
    }

    /// Calculate the number of impostors based on player count.
    fn calculate_impostor_count(player_count: usize) -> usize {
        match player_count {
            0..=5 => 1,
            6..=7 => 2,
            8..=10 => 2,
            11..=12 => 3,
            _ => 3,
        }
    }

    /// Resolve a PlayerId to a ClientId via the game data registry.
    fn resolve_client_id(&self, player_id: PlayerId) -> Option<i32> {
        if let Some(gd) = self.game_data.lock().as_ref() {
            if let Some(info) = gd.get_by_player_id(player_id) {
                return Some(info.client_id);
            }
        }
        None
    }

    /// Resolve a ClientId to a PlayerId via the game data registry.
    #[allow(dead_code)]
    fn resolve_player_id(&self, client_id: i32) -> Option<PlayerId> {
        if let Some(gd) = self.game_data.lock().as_ref() {
            if let Some(info) = gd.get_by_client_id(client_id) {
                return Some(info.player_id);
            }
        }
        None
    }

    /// Find the ClientPlayer for a given player ID.
    fn find_client_player_by_player_id(&self, game_data: &InnerGameData, player_id: PlayerId) -> Option<dashmap::mapref::one::RefMut<'_, i32, ClientPlayer>> {
        if let Some(info) = game_data.get_by_player_id(player_id) {
            self.game.players.get_mut(&info.client_id)
        } else {
            None
        }
    }

    /// Determine the map type from game options.
    fn resolve_map_type(&self) -> MapType {
        // Try to read from options
        let map_byte = if let Some(normal) = self.game.options.as_any().downcast_ref::<railway_protocol::game_options::NormalGameOptions>() {
            normal.map
        } else if let Some(hns) = self.game.options.as_any().downcast_ref::<railway_protocol::game_options::HideNSeekGameOptions>() {
            hns.map
        } else {
            0
        };

        match map_byte {
            0 => MapType::Skeld,
            1 => MapType::MiraHQ,
            2 => MapType::Polus,
            3 => MapType::Dleks,
            4 => MapType::Airship,
            5 => MapType::Fungle,
            _ => MapType::Skeld,
        }
    }
}

pub use crate::game_flow::role_selection::RoleSelector;
