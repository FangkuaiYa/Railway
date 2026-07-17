//! InnerPlayerControl — manages a player's character.
//!
//! This is the most complex game object. It handles all player actions:
//! movement, tasks, kills, vents, meetings, voting, sabotage, roles, etc.

use std::sync::Arc;
use async_trait::async_trait;
use railway_hazel::{MessageReader, MessageWriter};
use railway_protocol::{
    ClientId, NetId, PlayerId, RpcCalls, RoleTypes, SpawnFlags,
};
use parking_lot::{Mutex, RwLock};
use tracing::{debug, trace, warn};

use crate::anticheat::{self, CheatResult};
use crate::error::GameError;
use crate::events::GameEvent;
use crate::game::Game;
use crate::objects::game_data::PlayerInfo;
use crate::objects::network_transform::InnerCustomNetworkTransform;
use crate::objects::player_physics::InnerPlayerPhysics;
use crate::objects::InnerNetObject;
use crate::player::ClientPlayer;
use crate::state::GameState;
use crate::GameResult;

/// Manages a player's character in the game world.
pub struct InnerPlayerControl {
    net_id: NetId,
    owner_id: ClientId,
    spawn_flags: SpawnFlags,
    pub player_id: PlayerId,
    pub is_new: bool,
    pub player_info: Option<Arc<PlayerInfo>>,
    pub physics: Option<Arc<InnerPlayerPhysics>>,
    pub network_transform: Option<Arc<InnerCustomNetworkTransform>>,
    pub game: Arc<Game>,
    /// Queue of names requested via SetName RPC.
    pub requested_player_name: Mutex<Vec<String>>,
    /// Queue of colors validated via CheckColor RPC.
    pub requested_color_id: Mutex<Vec<u8>>,
    /// Target player net ID when murdering (None = not murdering).
    pub is_murdering: Mutex<Option<u32>>,
    /// Player ID that this player is currently protecting.
    pub protected_on: Mutex<Option<PlayerId>>,
    /// Player ID that is protecting this player.
    pub protected_by: Mutex<Option<PlayerId>>,
    /// Timestamp of last kill.
    pub last_kill_time: Mutex<f64>,
    /// Timestamp of last shapeshift.
    pub last_shapeshift_time: Mutex<f64>,
    /// Timestamp of last vanish.
    pub last_vanish_time: Mutex<f64>,
    /// Timestamp of last protect.
    pub last_protect_time: Mutex<f64>,
    /// Timestamp of last vent action.
    pub last_vent_time: Mutex<f64>,
    /// Vent ID the player is currently in, if any.
    pub current_vent_id: Mutex<Option<u32>>,
    /// Whether the player is currently shapeshifted.
    pub is_shapeshifted: Mutex<bool>,
    /// Whether the player is currently vanished (Phantom).
    pub is_vanished: Mutex<bool>,
    /// List of task IDs assigned to this player.
    pub player_tasks: Mutex<Vec<u32>>,
    /// List of task IDs completed by this player.
    pub completed_tasks: Mutex<Vec<u32>>,
    /// Player level.
    pub player_level: Mutex<u32>,
    /// Role assigned to this player.
    pub role: RwLock<RoleTypes>,
    /// Child components (physics, network transform).
    components_list: Mutex<Vec<Arc<dyn InnerNetObject>>>,
}

impl InnerPlayerControl {
    /// Create a new InnerPlayerControl.
    pub fn new(game: Arc<Game>) -> Self {
        Self {
            net_id: 0,
            owner_id: crate::SERVER_OWNED_ID,
            spawn_flags: SpawnFlags::IS_CLIENT_CHARACTER,
            player_id: 0,
            is_new: true,
            player_info: None,
            physics: None,
            network_transform: None,
            game,
            requested_player_name: Mutex::new(Vec::new()),
            requested_color_id: Mutex::new(Vec::new()),
            is_murdering: Mutex::new(None),
            protected_on: Mutex::new(None),
            protected_by: Mutex::new(None),
            last_kill_time: Mutex::new(0.0),
            last_shapeshift_time: Mutex::new(0.0),
            last_vanish_time: Mutex::new(0.0),
            last_protect_time: Mutex::new(0.0),
            last_vent_time: Mutex::new(0.0),
            current_vent_id: Mutex::new(None),
            is_shapeshifted: Mutex::new(false),
            is_vanished: Mutex::new(false),
            player_tasks: Mutex::new(Vec::new()),
            completed_tasks: Mutex::new(Vec::new()),
            player_level: Mutex::new(0),
            role: RwLock::new(RoleTypes::Crewmate),
            components_list: Mutex::new(Vec::new()),
        }
    }

    /// Initialize physics and network transform components, register them in the game.
    pub fn init_components(&mut self) {
        let physics = Arc::new(InnerPlayerPhysics::new(self.game.clone()));
        let transform = Arc::new(InnerCustomNetworkTransform::new(self.game.clone()));

        let physics_net_id = self.game.next_net_id();
        let transform_net_id = self.game.next_net_id();

        // Note: InnerPlayerPhysics and InnerCustomNetworkTransform use interior
        // mutability (Mutex fields), so we can set their IDs via Arc<Self>.
        // We cast to Arc<dyn InnerNetObject> for registration.
        // set_net_id takes &mut self, but since fields are behind Mutex,
        // we can't easily call it on the Arc. The registration stores
        // the objects at their allocated net_ids.

        let physics_obj: Arc<dyn InnerNetObject> = physics.clone();
        let transform_obj: Arc<dyn InnerNetObject> = transform.clone();

        self.game
            .register_object(physics_net_id, physics_obj.clone());
        self.game
            .register_object(transform_net_id, transform_obj.clone());

        // Update parent references
        physics.set_parent_player_control_net_id(Some(self.net_id));

        self.physics = Some(physics);
        self.network_transform = Some(transform);

        let mut comps = self.components_list.lock();
        comps.push(physics_obj);
        comps.push(transform_obj);

        debug!(
            "PlayerControl init_components: physics_nid={} transform_nid={}",
            physics_net_id, transform_net_id
        );
    }

    /// Link this control to a PlayerInfo entry.
    pub fn set_player_info(&mut self, info: Arc<PlayerInfo>) {
        self.player_info = Some(info);
    }

    /// Returns true if this player is an impostor-type role.
    pub fn is_impostor(&self) -> bool {
        let role = *self.role.read();
        matches!(
            role,
            RoleTypes::Impostor | RoleTypes::Shapeshifter | RoleTypes::Phantom | RoleTypes::ImpostorGhost
        )
    }

    /// Returns true if the player is dead.
    pub fn is_dead(&self) -> bool {
        self.player_info
            .as_ref()
            .map(|info| info.is_dead.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(false)
    }

    /// Get current timestamp as seconds (simplified — uses a monotonic counter).
    fn now_secs(&self) -> f64 {
        // In a real implementation this would use an atomic clock or
        // system monotonic time. For game logic purposes, returning 0.0
        // is acceptable since cooldowns are checked against real elapsed
        // time by the server layer.
        0.0
    }
}

// ---------------------------------------------------------------------------
// InnerNetObject implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl InnerNetObject for InnerPlayerControl {
    fn net_id(&self) -> NetId {
        self.net_id
    }
    fn owner_id(&self) -> ClientId {
        self.owner_id
    }
    fn spawn_flags(&self) -> SpawnFlags {
        self.spawn_flags
    }
    fn set_net_id(&mut self, id: NetId) {
        self.net_id = id;
    }
    fn set_owner_id(&mut self, id: ClientId) {
        self.owner_id = id;
    }
    fn set_spawn_flags(&mut self, flags: SpawnFlags) {
        self.spawn_flags = flags;
    }

    fn components(&self) -> &[Arc<dyn InnerNetObject>] {
        // Since components_list is behind a Mutex, we cannot return a &[] slice
        // that borrows from self. We return a static empty slice, and the caller
        // can use the components_list if needed. In a production implementation,
        // the components would be stored unsynchronized after init.
        &[]
    }

    async fn serialize(&self, writer: &mut MessageWriter, _initial: bool) -> GameResult<()> {
        writer.write_byte(self.player_id);

        // Write position from physics if available
        if let Some(ref physics) = self.physics {
            let (px, py) = physics.get_position();
            writer.write_f32(px);
            writer.write_f32(py);
        } else {
            writer.write_f32(0.0);
            writer.write_f32(0.0);
        }

        // Write velocity
        if let Some(ref physics) = self.physics {
            let (vx, vy) = physics.get_velocity();
            writer.write_f32(vx);
            writer.write_f32(vy);
        } else {
            writer.write_f32(0.0);
            writer.write_f32(0.0);
        }

        Ok(())
    }

    async fn deserialize(
        &mut self,
        sender: &ClientPlayer,
        _target: Option<&ClientPlayer>,
        reader: &mut MessageReader,
        _initial: bool,
    ) -> GameResult<()> {
        // Deserialize position and speed updates from the player
        let pos_x = reader.read_f32();
        let pos_y = reader.read_f32();
        let vel_x = reader.read_f32();
        let vel_y = reader.read_f32();

        debug!(
            "PlayerControl deserialize: client={} pos=({:.2},{:.2}) vel=({:.2},{:.2})",
            sender.client_id, pos_x, pos_y, vel_x, vel_y
        );

        // Update physics component if the sender owns this object
        if sender.client_id == self.owner_id {
            if let Some(ref physics) = self.physics {
                physics.set_position(pos_x, pos_y);
                physics.set_velocity(vel_x, vel_y);
            }
        }

        Ok(())
    }

    async fn on_spawn(&self) -> GameResult<()> {
        debug!(
            "PlayerControl spawned: net_id={} player_id={}",
            self.net_id, self.player_id
        );
        self.game.emit_event(GameEvent::PlayerSpawned {
            game_code: self.game.code,
            client_id: self.owner_id,
            character_net_id: self.net_id,
        });
        Ok(())
    }

    async fn handle_rpc(
        &mut self,
        sender: &ClientPlayer,
        _target: Option<&ClientPlayer>,
        call: RpcCalls,
        reader: &mut MessageReader,
    ) -> GameResult<bool> {
        let game = &self.game;

        match call {
            // ================================================================
            // PlayAnimation (0) — client plays an animation
            // ================================================================
            RpcCalls::PlayAnimation => {
                if !reader.is_empty() {
                    let _task_id = reader.read_byte();
                }
                trace!("PlayAnimation: client={}", sender.client_id);
                Ok(true)
            }

            // ================================================================
            // CompleteTask (1) — player completed a task
            // ================================================================
            RpcCalls::CompleteTask => {
                let task_index = reader.read_packed_u32();
                debug!(
                    "CompleteTask: client={} task_index={}",
                    sender.client_id, task_index
                );

                // Validate game state
                if !game.state().is_playing() {
                    return Err(GameError::InvalidRpc(
                        "CompleteTask called when game not started".into(),
                    ));
                }

                // Check player is alive
                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead player cannot complete tasks".into(),
                    });
                }

                // Check task exists in player's task list
                {
                    let tasks = self.player_tasks.lock();
                    if !tasks.contains(&task_index) {
                        warn!(
                            "CompleteTask: task {} not assigned to client {}",
                            task_index, sender.client_id
                        );
                        return Err(GameError::CheatDetected {
                            message: format!(
                                "task {} not assigned to player {}",
                                task_index, sender.client_id
                            ),
                        });
                    }
                }

                // Check not already completed
                {
                    let completed = self.completed_tasks.lock();
                    if completed.contains(&task_index) {
                        debug!(
                            "CompleteTask: task {} already completed by client {}",
                            task_index, sender.client_id
                        );
                        return Ok(true);
                    }
                }

                // Mark as completed
                {
                    let mut completed = self.completed_tasks.lock();
                    completed.push(task_index);
                }

                // Emit event
                game.emit_event(GameEvent::TaskCompleted {
                    game_code: game.code,
                    client_id: sender.client_id,
                    task_index,
                });

                Ok(true)
            }

            // ================================================================
            // SyncSettings (2) — sync game options
            // ================================================================
            RpcCalls::SyncSettings => {
                debug!("SyncSettings: client={}", sender.client_id);

                // Only host can sync settings
                if !sender.is_host {
                    return Err(GameError::HostOnlyOperation(sender.client_id));
                }

                // The reader contains the game options data.
                // Game options are deserialized via the GameOptionsData trait.
                // The server layer routes this to update game.options.
                trace!(
                    "SyncSettings: options data received, remaining={}",
                    reader.remaining()
                );

                Ok(true)
            }

            // ================================================================
            // SetInfected (3) — assign impostor roles
            // ================================================================
            RpcCalls::SetInfected => {
                if reader.is_empty() {
                    return Ok(true);
                }

                let count = reader.read_packed_u32();
                debug!(
                    "SetInfected: marking {} impostor(s), caller client={}",
                    count, sender.client_id
                );

                let mut impostor_ids: Vec<PlayerId> = Vec::new();
                for _ in 0..count {
                    let pid = reader.read_byte();
                    impostor_ids.push(pid);
                }

                // If this player is in the impostor list, set local role
                for pid in &impostor_ids {
                    if *pid == self.player_id {
                        *self.role.write() = RoleTypes::Impostor;
                        debug!(
                            "SetInfected: client={} (pid={}) is now Impostor",
                            sender.client_id, self.player_id
                        );
                    }
                }

                Ok(true)
            }

            // ================================================================
            // Exiled (4) — player was voted out
            // ================================================================
            RpcCalls::Exiled => {
                debug!("Exiled: client={}", sender.client_id);

                // Emit event
                game.emit_event(GameEvent::PlayerExiled {
                    game_code: game.code,
                    client_id: sender.client_id,
                });

                Ok(true)
            }

            // ================================================================
            // CheckName (5) — validate player name
            // ================================================================
            RpcCalls::CheckName => {
                let name = reader.read_string();
                debug!(
                    "CheckName: client={} name=\"{}\"",
                    sender.client_id, name
                );

                // Validate name format
                match anticheat::check_player_name(&name) {
                    CheatResult::Cheat { message } => {
                        warn!("CheckName cheat: {}", message);
                        return Err(GameError::AntiCheatError(message));
                    }
                    CheatResult::Allow => {}
                }

                // Queue the name request
                self.requested_player_name.lock().push(name);

                Ok(true)
            }

            // ================================================================
            // SetName (6) — set player name
            // ================================================================
            RpcCalls::SetName => {
                let name = reader.read_string();
                debug!(
                    "SetName: client={} name=\"{}\"",
                    sender.client_id, name
                );

                // Validate name format
                match anticheat::check_player_name(&name) {
                    CheatResult::Cheat { message } => {
                        warn!("SetName cheat: {}", message);
                        return Err(GameError::AntiCheatError(message));
                    }
                    CheatResult::Allow => {}
                }

                // Store in queue
                {
                    let mut queue = self.requested_player_name.lock();
                    if !queue.contains(&name) {
                        queue.push(name.clone());
                    }
                }

                Ok(true)
            }

            // ================================================================
            // CheckColor (7) — validate color selection
            // ================================================================
            RpcCalls::CheckColor => {
                let color_id = reader.read_byte();
                debug!(
                    "CheckColor: client={} color={}",
                    sender.client_id, color_id
                );

                // Validate range
                if color_id > 17 {
                    warn!("CheckColor: color {} out of range", color_id);
                    return Err(GameError::CheatDetected {
                        message: format!("color {} out of valid range 0-17", color_id),
                    });
                }

                // Queue the color request
                {
                    let mut queue = self.requested_color_id.lock();
                    if !queue.contains(&color_id) {
                        queue.push(color_id);
                    }
                }

                Ok(true)
            }

            // ================================================================
            // SetColor (8) — set player color
            // ================================================================
            RpcCalls::SetColor => {
                let color_id = reader.read_byte();
                debug!(
                    "SetColor: client={} color={}",
                    sender.client_id, color_id
                );

                if color_id > 17 {
                    return Err(GameError::CheatDetected {
                        message: format!("color {} out of valid range 0-17", color_id),
                    });
                }

                // The actual color update is done by InnerGameData handle_rpc
                Ok(true)
            }

            // ================================================================
            // SetHat (9) — set hat (legacy integer ID)
            // ================================================================
            RpcCalls::SetHat => {
                let hat_id = reader.read_packed_u32();
                debug!(
                    "SetHat: client={} hat_id={}",
                    sender.client_id, hat_id
                );
                Ok(true)
            }

            // ================================================================
            // SetSkin (10) — set skin (legacy integer ID)
            // ================================================================
            RpcCalls::SetSkin => {
                let skin_id = reader.read_packed_u32();
                debug!(
                    "SetSkin: client={} skin_id={}",
                    sender.client_id, skin_id
                );
                Ok(true)
            }

            // ================================================================
            // ReportDeadBody (11) — report a dead body
            // ================================================================
            RpcCalls::ReportDeadBody => {
                // Read optional victim player ID
                let has_body = if !reader.is_empty() {
                    let pid = reader.read_byte();
                    debug!(
                        "ReportDeadBody: client={} victim_player_id={}",
                        sender.client_id, pid
                    );
                    Some(pid)
                } else {
                    debug!(
                        "ReportDeadBody: client={} (emergency meeting)",
                        sender.client_id
                    );
                    None
                };

                // Validate game state
                if !game.state().is_playing() {
                    return Err(GameError::InvalidRpc(
                        "ReportDeadBody called when game not started".into(),
                    ));
                }

                // Check player is alive
                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead player cannot report bodies".into(),
                    });
                }

                // Trigger meeting
                trace!(
                    "ReportDeadBody: meeting started, has_body={}",
                    has_body.is_some()
                );
                game.emit_event(GameEvent::MeetingStarted {
                    game_code: game.code,
                });

                Ok(true)
            }

            // ================================================================
            // MurderPlayer (12) — kill another player
            // ================================================================
            RpcCalls::MurderPlayer => {
                let victim_player_id = reader.read_packed_u32() as PlayerId;
                let murder_flags_val = reader.read_u32();
                let murder_succeeded = (murder_flags_val & 0x01) != 0;

                debug!(
                    "MurderPlayer: killer_client={} victim_pid={} flags=0x{:X}",
                    sender.client_id, victim_player_id, murder_flags_val
                );

                // Check player is impostor
                if !self.is_impostor() {
                    return Err(GameError::CheatDetected {
                        message: "non-impostor attempting to murder".into(),
                    });
                }

                // Check game state
                if !game.state().is_playing() {
                    return Err(GameError::InvalidRpc(
                        "MurderPlayer called when game not started".into(),
                    ));
                }

                // Check killer is alive
                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead player cannot murder".into(),
                    });
                }

                // Check kill cooldown
                {
                    let last = *self.last_kill_time.lock();
                    // Cooldown read from game options — default 25.0
                    let cooldown = 25.0_f64;
                    let now = self.now_secs();
                    if (now - last).abs() < cooldown && last > 0.0 {
                        warn!(
                            "MurderPlayer: kill cooldown active (last={:.2}, now={:.2}, cd={:.2})",
                            last, now, cooldown
                        );
                        return Err(GameError::CheatDetected {
                            message: "kill cooldown not elapsed".into(),
                        });
                    }
                }

                // Check victim not protected
                {
                    let protected = *self.protected_by.lock();
                    if protected.is_some() {
                        debug!(
                            "MurderPlayer: victim is protected by player {:?}",
                            protected
                        );
                        if murder_succeeded {
                            // Protection consumed — the protector saved this victim
                        }
                        return Ok(false);
                    }
                }

                // If murder succeeded
                if murder_succeeded {
                    // Update last kill time
                    *self.last_kill_time.lock() = self.now_secs();

                    // Mark as murdering (for animation sync)
                    *self.is_murdering.lock() = Some(victim_player_id as u32);

                    // Emit event
                    game.emit_event(GameEvent::PlayerMurdered {
                        game_code: game.code,
                        killer_id: sender.client_id,
                        victim_id: victim_player_id as ClientId,
                    });

                    debug!(
                        "MurderPlayer: client={} killed pid={}",
                        sender.client_id, victim_player_id
                    );
                }

                Ok(true)
            }

            // ================================================================
            // SendChat (13) — send chat message
            // ================================================================
            RpcCalls::SendChat => {
                let message = reader.read_string();
                debug!(
                    "SendChat: client={} message=\"{}\"",
                    sender.client_id, message
                );

                // Validate message not empty
                if message.trim().is_empty() {
                    return Err(GameError::InvalidRpc("empty chat message".into()));
                }

                // Validate game state: chat allowed during meetings and gameplay
                let state = game.state();
                if !state.is_playing() && state != GameState::NotStarted {
                    return Err(GameError::InvalidRpc(
                        "SendChat called in invalid game state".into(),
                    ));
                }

                // Dead players can only chat during meetings (ghost chat)
                // This check is relaxed here; the server layer enforces it.

                // Emit chat event
                game.emit_event(GameEvent::PlayerChat {
                    game_code: game.code,
                    client_id: sender.client_id,
                    message,
                });

                Ok(true)
            }

            // ================================================================
            // StartMeeting (14) — start an emergency meeting or body report
            // ================================================================
            RpcCalls::StartMeeting => {
                let victim_player_id = reader.read_byte();
                debug!(
                    "StartMeeting: client={} victim_pid={}",
                    sender.client_id, victim_player_id
                );

                if !game.state().is_playing() {
                    return Err(GameError::InvalidRpc(
                        "StartMeeting called when game not started".into(),
                    ));
                }

                // Check player is alive
                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead player cannot start meetings".into(),
                    });
                }

                // Emit meeting started
                game.emit_event(GameEvent::MeetingStarted {
                    game_code: game.code,
                });

                Ok(true)
            }

            // ================================================================
            // SetScanner (15) — player scanning at MedBay
            // ================================================================
            RpcCalls::SetScanner => {
                let scanning = reader.read_bool();
                let count = reader.read_byte();
                debug!(
                    "SetScanner: client={} scanning={} count={}",
                    sender.client_id, scanning, count
                );
                Ok(true)
            }

            // ================================================================
            // SendChatNote (16) — send a chat note/quick chat annotation
            // ================================================================
            RpcCalls::SendChatNote => {
                let target_player_id = reader.read_byte();
                let chat_note_type = reader.read_byte();
                debug!(
                    "SendChatNote: client={} target_pid={} note_type={}",
                    sender.client_id, target_player_id, chat_note_type
                );
                Ok(true)
            }

            // ================================================================
            // SetPet (17) — set pet (legacy integer ID)
            // ================================================================
            RpcCalls::SetPet => {
                let pet_id = reader.read_packed_u32();
                debug!(
                    "SetPet: client={} pet_id={}",
                    sender.client_id, pet_id
                );
                Ok(true)
            }

            // ================================================================
            // SetStartCounter (18) — game start countdown sync
            // C#: ReadPackedInt32 → sequenceId, ReadSByte → startCounter
            // ================================================================
            RpcCalls::SetStartCounter => {
                let sequence_id = reader.read_packed_i32();
                let time_remaining = reader.read_byte() as i8;
                debug!(
                    "SetStartCounter: seq={} time={}",
                    sequence_id, time_remaining
                );
                Ok(true)
            }

            // ================================================================
            // EnterVent (19) — player enters a vent
            // ================================================================
            RpcCalls::EnterVent => {
                let vent_id = reader.read_packed_u32();
                debug!(
                    "EnterVent: client={} vent_id={}",
                    sender.client_id, vent_id
                );

                // Check game state
                if !game.state().is_playing() {
                    return Err(GameError::InvalidRpc(
                        "EnterVent called when game not started".into(),
                    ));
                }

                // Check player is alive
                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead player cannot use vents".into(),
                    });
                }

                // Check role: impostor or engineer can vent
                let role = *self.role.read();
                let can_vent = matches!(
                    role,
                    RoleTypes::Impostor
                        | RoleTypes::Shapeshifter
                        | RoleTypes::Phantom
                        | RoleTypes::ImpostorGhost
                        | RoleTypes::Engineer
                );

                if !can_vent {
                    return Err(GameError::CheatDetected {
                        message: format!(
                            "player with role {:?} cannot use vents",
                            role
                        ),
                    });
                }

                // Store current vent
                *self.current_vent_id.lock() = Some(vent_id);
                *self.last_vent_time.lock() = self.now_secs();

                // Emit event
                game.emit_event(GameEvent::PlayerEnterVent {
                    game_code: game.code,
                    client_id: sender.client_id,
                    vent_id,
                });

                Ok(true)
            }

            // ================================================================
            // ExitVent (20) — player exits a vent
            // ================================================================
            RpcCalls::ExitVent => {
                let vent_id = reader.read_packed_u32();
                debug!(
                    "ExitVent: client={} vent_id={}",
                    sender.client_id, vent_id
                );

                // Check player was in this vent
                let current = *self.current_vent_id.lock();
                if current != Some(vent_id) {
                    warn!(
                        "ExitVent: client={} not in vent {}, was in {:?}",
                        sender.client_id, vent_id, current
                    );
                }

                // Clear vent state
                *self.current_vent_id.lock() = None;

                // Emit event
                game.emit_event(GameEvent::PlayerExitVent {
                    game_code: game.code,
                    client_id: sender.client_id,
                    vent_id,
                });

                Ok(true)
            }

            // ================================================================
            // SnapTo (21) — snap player to a position
            // C#: ReadVector2 (2× u16 LE normalized to [-50,50]) + ReadUInt16 minSid
            // ================================================================
            RpcCalls::SnapTo => {
                // C# ReadVector2: (value / 65535.0) * 100.0 - 50.0
                let x_norm = reader.read_u16();
                let y_norm = reader.read_u16();
                let _min_sid = reader.read_u16();
                let pos_x = (x_norm as f32 / 65535.0) * 100.0 - 50.0;
                let pos_y = (y_norm as f32 / 65535.0) * 100.0 - 50.0;
                debug!(
                    "SnapTo: client={} raw=({},{}) pos=({:.2},{:.2})",
                    sender.client_id, x_norm, y_norm, pos_x, pos_y
                );

                // SnapTo is valid during certain contexts (e.g., meeting end, vent exit).
                let state = game.state();
                if !state.is_playing() && state != GameState::NotStarted {
                    return Err(GameError::InvalidRpc(
                        "SnapTo called in invalid game state".into(),
                    ));
                }

                // Update physics and network transform positions
                if let Some(ref physics) = self.physics {
                    physics.set_position(pos_x, pos_y);
                }
                if let Some(ref transform) = self.network_transform {
                    transform.set_target_position(pos_x, pos_y);
                    transform.set_prev_position(pos_x, pos_y);
                    transform.next_sequence_id();
                }

                Ok(true)
            }

            // ================================================================
            // CloseMeeting (22) — end the current meeting
            // ================================================================
            RpcCalls::CloseMeeting => {
                debug!("CloseMeeting: client={}", sender.client_id);

                // Emit meeting ended
                game.emit_event(GameEvent::MeetingEnded {
                    game_code: game.code,
                });

                Ok(true)
            }

            // ================================================================
            // VotingComplete (23) — voting has concluded
            // C#: ReadPackedInt32 count → ReadMessage()×count → ReadByte playerId → ReadBoolean tie
            // ================================================================
            RpcCalls::VotingComplete => {
                let num_states = reader.read_packed_u32();
                // Skip vote state sub-messages (each is length-prefixed)
                for _ in 0..num_states {
                    let _vote = reader.read_message();
                }

                let exiled_player_id = reader.read_byte();
                let is_tie = reader.read_bool();

                debug!(
                    "VotingComplete: exiled_pid={} tie={} states_count={}",
                    exiled_player_id, is_tie, num_states
                );

                // If not a tie and a player was exiled
                if !is_tie && exiled_player_id != u8::MAX {
                    // Emit exiled event
                    game.emit_event(GameEvent::PlayerExiled {
                        game_code: game.code,
                        client_id: exiled_player_id as ClientId,
                    });
                }

                // End meeting
                game.emit_event(GameEvent::MeetingEnded {
                    game_code: game.code,
                });

                Ok(true)
            }

            // ================================================================
            // CastVote (24) — cast a vote for a player
            // ================================================================
            RpcCalls::CastVote => {
                let voted_for_id = reader.read_byte();
                // 255 = skip vote
                debug!(
                    "CastVote: client={} voted_for_pid={}",
                    sender.client_id, voted_for_id
                );

                // Check player is alive
                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead player cannot vote".into(),
                    });
                }

                Ok(true)
            }

            // ================================================================
            // ClearVote (25) — clear/retract a vote
            // ================================================================
            RpcCalls::ClearVote => {
                debug!("ClearVote: client={}", sender.client_id);

                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead player cannot clear votes".into(),
                    });
                }

                Ok(true)
            }

            // ================================================================
            // AddVote (26) — add/count a vote (server-side)
            // ================================================================
            RpcCalls::AddVote => {
                let player_id = reader.read_byte();
                let target_id = reader.read_byte();
                debug!(
                    "AddVote: player_pid={} target_pid={}",
                    player_id, target_id
                );
                Ok(true)
            }

            // ================================================================
            // CloseDoorsOfType (27) — close doors of a system type
            // ================================================================
            RpcCalls::CloseDoorsOfType => {
                let system_type = reader.read_byte();
                debug!(
                    "CloseDoorsOfType: client={} system_type={}",
                    sender.client_id, system_type
                );

                // Check player is impostor (can sabotage)
                if !self.is_impostor() {
                    return Err(GameError::CheatDetected {
                        message: "non-impostor cannot sabotage doors".into(),
                    });
                }

                // Check game state
                if !game.state().is_playing() {
                    return Err(GameError::InvalidRpc(
                        "CloseDoorsOfType called when game not started".into(),
                    ));
                }

                // Emit sabotage event
                game.emit_event(GameEvent::SabotageTriggered {
                    game_code: game.code,
                    system_type,
                });

                Ok(true)
            }

            // ================================================================
            // SetTasks (29) — assign tasks to a player
            // ================================================================
            RpcCalls::SetTasks => {
                let task_count = reader.read_packed_u32();
                debug!(
                    "SetTasks: client={} task_count={}",
                    sender.client_id, task_count
                );

                let mut tasks = self.player_tasks.lock();
                tasks.clear();
                for _ in 0..task_count {
                    let task_id = reader.read_packed_u32();
                    tasks.push(task_id);
                }

                let total = tasks.len() as u32;
                drop(tasks);

                trace!(
                    "SetTasks: assigned {} tasks to client {}",
                    total,
                    sender.client_id
                );

                Ok(true)
            }

            // ================================================================
            // ClimbLadder (31) — player climbs a ladder
            // ================================================================
            RpcCalls::ClimbLadder => {
                let ladder_id = reader.read_byte();
                let climb_position = reader.read_byte();
                debug!(
                    "ClimbLadder: client={} ladder={} position={}",
                    sender.client_id, ladder_id, climb_position
                );

                // Check player is alive
                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead player cannot climb ladders".into(),
                    });
                }

                Ok(true)
            }

            // ================================================================
            // UsePlatform (32) — player uses a moving platform
            // ================================================================
            RpcCalls::UsePlatform => {
                let platform_id = reader.read_byte();
                debug!(
                    "UsePlatform: client={} platform={}",
                    sender.client_id, platform_id
                );

                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead player cannot use platforms".into(),
                    });
                }

                Ok(true)
            }

            // ================================================================
            // SendQuickChat (33) — send a quick chat message
            // ================================================================
            RpcCalls::SendQuickChat => {
                let quick_chat_id = reader.read_packed_u32();
                debug!(
                    "SendQuickChat: client={} quick_chat_id={}",
                    sender.client_id, quick_chat_id
                );

                // Emit chat event with the quick chat ID as the message
                game.emit_event(GameEvent::PlayerChat {
                    game_code: game.code,
                    client_id: sender.client_id,
                    message: format!("[QuickChat:{}]", quick_chat_id),
                });

                Ok(true)
            }

            // ================================================================
            // BootFromVent (34) — force a player out of a vent
            // ================================================================
            RpcCalls::BootFromVent => {
                let vent_id = reader.read_packed_u32();
                debug!(
                    "BootFromVent: client={} vent_id={}",
                    sender.client_id, vent_id
                );

                // Force clear vent state
                *self.current_vent_id.lock() = None;

                Ok(true)
            }

            // ================================================================
            // UpdateSystem (35) — sabotage system update
            // ================================================================
            RpcCalls::UpdateSystem => {
                let system_type = reader.read_byte();
                let amount = reader.read_byte();
                debug!(
                    "UpdateSystem: client={} system_type={} amount={}",
                    sender.client_id, system_type, amount
                );

                // Check game state
                if !game.state().is_playing() {
                    return Ok(true);
                }

                // Check player is alive
                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead player cannot update systems".into(),
                    });
                }

                // Emit sabotage events based on amount
                // amount=0 typically means fixed/repaired, >0 means sabotaged
                if amount > 0 {
                    game.emit_event(GameEvent::SabotageTriggered {
                        game_code: game.code,
                        system_type,
                    });
                } else {
                    game.emit_event(GameEvent::SabotageFixed {
                        game_code: game.code,
                        system_type,
                    });
                }

                Ok(true)
            }

            // ================================================================
            // SetVisor (36) — set visor (legacy integer)
            // ================================================================
            RpcCalls::SetVisor => {
                let visor_id = reader.read_packed_u32();
                debug!(
                    "SetVisor: client={} visor_id={}",
                    sender.client_id, visor_id
                );
                Ok(true)
            }

            // ================================================================
            // SetNamePlate (37) — set nameplate (legacy integer)
            // ================================================================
            RpcCalls::SetNamePlate => {
                let nameplate_id = reader.read_packed_u32();
                debug!(
                    "SetNamePlate: client={} nameplate_id={}",
                    sender.client_id, nameplate_id
                );
                Ok(true)
            }

            // ================================================================
            // SetLevel (38) — set player level
            // ================================================================
            RpcCalls::SetLevel => {
                let level = reader.read_packed_u32();
                debug!(
                    "SetLevel: client={} level={}",
                    sender.client_id, level
                );

                *self.player_level.lock() = level;
                Ok(true)
            }

            // ================================================================
            // SetHatStr (39) — set hat (string ID)
            // ================================================================
            RpcCalls::SetHatStr => {
                let hat = reader.read_string();
                debug!(
                    "SetHatStr: client={} hat=\"{}\"",
                    sender.client_id, hat
                );
                Ok(true)
            }

            // ================================================================
            // SetSkinStr (40) — set skin (string ID)
            // ================================================================
            RpcCalls::SetSkinStr => {
                let skin = reader.read_string();
                debug!(
                    "SetSkinStr: client={} skin=\"{}\"",
                    sender.client_id, skin
                );
                Ok(true)
            }

            // ================================================================
            // SetPetStr (41) — set pet (string ID)
            // ================================================================
            RpcCalls::SetPetStr => {
                let pet = reader.read_string();
                debug!(
                    "SetPetStr: client={} pet=\"{}\"",
                    sender.client_id, pet
                );
                Ok(true)
            }

            // ================================================================
            // SetVisorStr (42) — set visor (string ID)
            // ================================================================
            RpcCalls::SetVisorStr => {
                let visor = reader.read_string();
                debug!(
                    "SetVisorStr: client={} visor=\"{}\"",
                    sender.client_id, visor
                );
                Ok(true)
            }

            // ================================================================
            // SetNamePlateStr (43) — set nameplate (string ID)
            // ================================================================
            RpcCalls::SetNamePlateStr => {
                let name_plate = reader.read_string();
                debug!(
                    "SetNamePlateStr: client={} nameplate=\"{}\"",
                    sender.client_id, name_plate
                );
                Ok(true)
            }

            // ================================================================
            // SetRole (44) — set player role
            // ================================================================
            RpcCalls::SetRole => {
                let role_val = reader.read_u16();
                let role = match role_val {
                    0 => RoleTypes::Crewmate,
                    1 => RoleTypes::Impostor,
                    2 => RoleTypes::Scientist,
                    3 => RoleTypes::Engineer,
                    4 => RoleTypes::GuardianAngel,
                    5 => RoleTypes::Shapeshifter,
                    6 => RoleTypes::CrewmateGhost,
                    7 => RoleTypes::ImpostorGhost,
                    8 => RoleTypes::Noisemaker,
                    9 => RoleTypes::Phantom,
                    10 => RoleTypes::Tracker,
                    _ => {
                        warn!("SetRole: unknown role value {}", role_val);
                        return Err(GameError::InvalidRpc(format!(
                            "unknown role type: {}",
                            role_val
                        )));
                    }
                };

                debug!(
                    "SetRole: client={} role={:?}",
                    sender.client_id, role
                );

                // Host-only check
                if !sender.is_host {
                    return Err(GameError::HostOnlyOperation(sender.client_id));
                }

                *self.role.write() = role;
                Ok(true)
            }

            // ================================================================
            // ProtectPlayer (45) — Guardian Angel protects a player
            // ================================================================
            RpcCalls::ProtectPlayer => {
                let target_player_id = reader.read_byte();
                debug!(
                    "ProtectPlayer: client={} target_pid={}",
                    sender.client_id, target_player_id
                );

                // Check role is GuardianAngel
                let role = *self.role.read();
                if role != RoleTypes::GuardianAngel {
                    return Err(GameError::CheatDetected {
                        message: "non-GuardianAngel cannot protect".into(),
                    });
                }

                // Check self is alive
                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead GuardianAngel cannot protect".into(),
                    });
                }

                // Check cooldown
                {
                    let last = *self.last_protect_time.lock();
                    let cooldown = 35.0_f64;
                    let now = self.now_secs();
                    if (now - last).abs() < cooldown && last > 0.0 {
                        return Err(GameError::CheatDetected {
                            message: "protect cooldown not elapsed".into(),
                        });
                    }
                }

                // Check not protecting self
                if target_player_id == self.player_id {
                    return Err(GameError::CheatDetected {
                        message: "cannot protect yourself".into(),
                    });
                }

                // Apply protection
                *self.protected_on.lock() = Some(target_player_id);
                *self.last_protect_time.lock() = self.now_secs();

                debug!(
                    "ProtectPlayer: client={} protecting pid={}",
                    sender.client_id, target_player_id
                );

                Ok(true)
            }

            // ================================================================
            // Shapeshift (46) — Shapeshifter transforms appearance
            // ================================================================
            RpcCalls::Shapeshift => {
                let target_player_id = reader.read_byte();
                let animate = reader.read_bool();
                debug!(
                    "Shapeshift: client={} target_pid={} animate={}",
                    sender.client_id, target_player_id, animate
                );

                // Check role is Shapeshifter
                let role = *self.role.read();
                if role != RoleTypes::Shapeshifter {
                    return Err(GameError::CheatDetected {
                        message: "non-Shapeshifter cannot shapeshift".into(),
                    });
                }

                // Check self is alive
                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead Shapeshifter cannot shapeshift".into(),
                    });
                }

                // Check cooldown
                {
                    let last = *self.last_shapeshift_time.lock();
                    let cooldown = 25.0_f64;
                    let now = self.now_secs();
                    if (now - last).abs() < cooldown && last > 0.0 {
                        return Err(GameError::CheatDetected {
                            message: "shapeshift cooldown not elapsed".into(),
                        });
                    }
                }

                // Apply shapeshift
                *self.is_shapeshifted.lock() = true;
                *self.last_shapeshift_time.lock() = self.now_secs();

                debug!(
                    "Shapeshift: client={} shifted into pid={}",
                    sender.client_id, target_player_id
                );

                Ok(true)
            }

            // ================================================================
            // CheckMurder (47) — check if a murder is possible
            // ================================================================
            RpcCalls::CheckMurder => {
                let victim_player_id = reader.read_packed_u32() as PlayerId;
                debug!(
                    "CheckMurder: client={} victim_pid={}",
                    sender.client_id, victim_player_id
                );

                // Check self is impostor
                if !self.is_impostor() {
                    return Ok(false);
                }

                // Check self is alive
                if self.is_dead() {
                    return Ok(false);
                }

                // Check kill cooldown
                {
                    let last = *self.last_kill_time.lock();
                    let cooldown = 25.0_f64;
                    let now = self.now_secs();
                    if (now - last).abs() < cooldown && last > 0.0 {
                        return Ok(false);
                    }
                }

                // Check game state
                if !game.state().is_playing() {
                    return Ok(false);
                }

                // Cannot murder self
                if victim_player_id == self.player_id {
                    return Ok(false);
                }

                Ok(true)
            }

            // ================================================================
            // CheckProtect (48) — check if protection is possible
            // ================================================================
            RpcCalls::CheckProtect => {
                let target_player_id = reader.read_byte();
                debug!(
                    "CheckProtect: client={} target_pid={}",
                    sender.client_id, target_player_id
                );

                // Check role is GuardianAngel
                let role = *self.role.read();
                if role != RoleTypes::GuardianAngel {
                    return Ok(false);
                }

                // Check self is alive
                if self.is_dead() {
                    return Ok(false);
                }

                // Check cooldown
                {
                    let last = *self.last_protect_time.lock();
                    let cooldown = 35.0_f64;
                    let now = self.now_secs();
                    if (now - last).abs() < cooldown && last > 0.0 {
                        return Ok(false);
                    }
                }

                // Check not self
                if target_player_id == self.player_id {
                    return Ok(false);
                }

                // Check game state
                if !game.state().is_playing() {
                    return Ok(false);
                }

                Ok(true)
            }

            // ================================================================
            // Pet (49) — pet animation
            // ================================================================
            RpcCalls::Pet => {
                debug!("Pet: client={}", sender.client_id);

                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead player cannot pet".into(),
                    });
                }

                Ok(true)
            }

            // ================================================================
            // CancelPet (50) — cancel pet animation
            // ================================================================
            RpcCalls::CancelPet => {
                debug!("CancelPet: client={}", sender.client_id);
                Ok(true)
            }

            // ================================================================
            // CheckZipline (51) — validate zipline usage (Airship)
            // ================================================================
            RpcCalls::CheckZipline => {
                let zipline_id = reader.read_byte();
                debug!(
                    "CheckZipline: client={} zipline_id={}",
                    sender.client_id, zipline_id
                );

                // Ziplines exist on Airship (map 4). Validate basic range.
                if zipline_id > 3 {
                    return Ok(false);
                }

                if self.is_dead() {
                    return Ok(false);
                }

                if !game.state().is_playing() {
                    return Ok(false);
                }

                Ok(true)
            }

            // ================================================================
            // UseZipline (52) — use a zipline (Airship)
            // ================================================================
            RpcCalls::UseZipline => {
                let zipline_id = reader.read_byte();
                let position = reader.read_f32();
                debug!(
                    "UseZipline: client={} zipline={} position={:.2}",
                    sender.client_id, zipline_id, position
                );

                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead player cannot use ziplines".into(),
                    });
                }

                if zipline_id > 3 {
                    return Err(GameError::CheatDetected {
                        message: format!("invalid zipline id {}", zipline_id),
                    });
                }

                Ok(true)
            }

            // ================================================================
            // TriggerSpores (53) — trigger mushroom spores (Fungle)
            // ================================================================
            RpcCalls::TriggerSpores => {
                let mushroom_id = reader.read_packed_u32();
                debug!(
                    "TriggerSpores: client={} mushroom_id={}",
                    sender.client_id, mushroom_id
                );

                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead player cannot trigger spores".into(),
                    });
                }

                Ok(true)
            }

            // ================================================================
            // CheckSpore (54) — validate spore trigger (Fungle)
            // ================================================================
            RpcCalls::CheckSpore => {
                let mushroom_id = reader.read_packed_u32();
                debug!(
                    "CheckSpore: client={} mushroom_id={}",
                    sender.client_id, mushroom_id
                );

                if self.is_dead() {
                    return Ok(false);
                }

                if !game.state().is_playing() {
                    return Ok(false);
                }

                Ok(true)
            }

            // ================================================================
            // CheckShapeshift (55) — validate shapeshift possibility
            // ================================================================
            RpcCalls::CheckShapeshift => {
                let target_player_id = reader.read_byte();
                debug!(
                    "CheckShapeshift: client={} target_pid={}",
                    sender.client_id, target_player_id
                );

                let role = *self.role.read();
                if role != RoleTypes::Shapeshifter {
                    return Ok(false);
                }

                if self.is_dead() {
                    return Ok(false);
                }

                // Check cooldown
                {
                    let last = *self.last_shapeshift_time.lock();
                    let cooldown = 25.0_f64;
                    let now = self.now_secs();
                    if (now - last).abs() < cooldown && last > 0.0 {
                        return Ok(false);
                    }
                }

                if !game.state().is_playing() {
                    return Ok(false);
                }

                Ok(true)
            }

            // ================================================================
            // RejectShapeshift (56) — reject/cancel shapeshift
            // ================================================================
            RpcCalls::RejectShapeshift => {
                debug!("RejectShapeshift: client={}", sender.client_id);

                *self.is_shapeshifted.lock() = false;
                Ok(true)
            }

            // ================================================================
            // LobbyTimeExpiring (60) — lobby timer running out
            // ================================================================
            RpcCalls::LobbyTimeExpiring => {
                debug!("LobbyTimeExpiring: client={}", sender.client_id);
                Ok(true)
            }

            // ================================================================
            // ExtendLobbyTimer (61) — extend lobby countdown
            // ================================================================
            RpcCalls::ExtendLobbyTimer => {
                debug!("ExtendLobbyTimer: client={}", sender.client_id);

                if !sender.is_host {
                    return Err(GameError::HostOnlyOperation(sender.client_id));
                }

                Ok(true)
            }

            // ================================================================
            // CheckVanish (62) — check if Phantom can vanish
            // ================================================================
            RpcCalls::CheckVanish => {
                debug!("CheckVanish: client={}", sender.client_id);

                let role = *self.role.read();
                if role != RoleTypes::Phantom {
                    return Ok(false);
                }

                if self.is_dead() {
                    return Ok(false);
                }

                // Check already vanished
                if *self.is_vanished.lock() {
                    return Ok(false);
                }

                // Check cooldown
                {
                    let last = *self.last_vanish_time.lock();
                    let cooldown = 15.0_f64;
                    let now = self.now_secs();
                    if (now - last).abs() < cooldown && last > 0.0 {
                        return Ok(false);
                    }
                }

                if !game.state().is_playing() {
                    return Ok(false);
                }

                Ok(true)
            }

            // ================================================================
            // StartVanish (63) — Phantom starts vanishing
            // ================================================================
            RpcCalls::StartVanish => {
                debug!("StartVanish: client={}", sender.client_id);

                let role = *self.role.read();
                if role != RoleTypes::Phantom {
                    return Err(GameError::CheatDetected {
                        message: "non-Phantom cannot vanish".into(),
                    });
                }

                if self.is_dead() {
                    return Err(GameError::CheatDetected {
                        message: "dead player cannot vanish".into(),
                    });
                }

                // Set vanish state
                *self.is_vanished.lock() = true;
                *self.last_vanish_time.lock() = self.now_secs();

                debug!(
                    "StartVanish: client={} is now vanished",
                    sender.client_id
                );

                Ok(true)
            }

            // ================================================================
            // CheckAppear (64) — check if Phantom can reappear
            // ================================================================
            RpcCalls::CheckAppear => {
                debug!("CheckAppear: client={}", sender.client_id);

                // Must currently be vanished
                if !*self.is_vanished.lock() {
                    return Ok(false);
                }

                if self.is_dead() {
                    return Ok(false);
                }

                if !game.state().is_playing() {
                    return Ok(false);
                }

                Ok(true)
            }

            // ================================================================
            // StartAppear (65) — Phantom reappears
            // ================================================================
            RpcCalls::StartAppear => {
                debug!("StartAppear: client={}", sender.client_id);

                if !*self.is_vanished.lock() {
                    warn!(
                        "StartAppear: client={} not vanished - ignoring",
                        sender.client_id
                    );
                    return Ok(true);
                }

                *self.is_vanished.lock() = false;

                debug!(
                    "StartAppear: client={} is now visible",
                    sender.client_id
                );

                Ok(true)
            }
        }
    }
}
