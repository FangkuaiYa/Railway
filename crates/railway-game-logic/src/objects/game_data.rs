//! InnerGameData — manages the PlayerInfo registry for a game.
//!
//! Maps player IDs to PlayerInfo objects and client IDs to PlayerInfo objects.
//! Handles player-data RPCs: name, color, cosmetics, roles.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use async_trait::async_trait;
use dashmap::DashMap;
use railway_hazel::{MessageReader, MessageWriter};
use railway_protocol::{ClientId, NetId, PlayerId, RpcCalls, SpawnFlags, RoleTypes};
use parking_lot::Mutex;
use tracing::{debug, trace, warn};

use crate::anticheat;
use crate::error::GameError;
use crate::objects::InnerNetObject;
use crate::player::ClientPlayer;
use crate::GameResult;

/// Manages PlayerInfo objects for a game.
pub struct InnerGameData {
    net_id: NetId,
    owner_id: ClientId,
    spawn_flags: SpawnFlags,
    /// PlayerInfo objects keyed by player ID (slot 0-14).
    players: DashMap<PlayerId, Arc<PlayerInfo>>,
    /// PlayerInfo objects keyed by client ID.
    players_by_client: DashMap<ClientId, Arc<PlayerInfo>>,
}

impl InnerGameData {
    pub fn new() -> Self {
        Self {
            net_id: 0,
            owner_id: crate::SERVER_OWNED_ID,
            spawn_flags: SpawnFlags::NONE,
            players: DashMap::new(),
            players_by_client: DashMap::new(),
        }
    }

    /// Add a PlayerInfo to the registry. Returns false if the slot is taken.
    pub fn add_player(&self, player_info: Arc<PlayerInfo>) -> bool {
        let player_id = player_info.player_id;
        let client_id = player_info.client_id;

        if self.players.contains_key(&player_id) {
            return false;
        }

        self.players.insert(player_id, player_info.clone());
        self.players_by_client.insert(client_id, player_info);
        true
    }

    /// Remove a PlayerInfo from the registry.
    pub fn remove_player(&self, player_id: PlayerId) {
        if let Some((_, info)) = self.players.remove(&player_id) {
            self.players_by_client.remove(&info.client_id);
        }
    }

    /// Get a PlayerInfo by player ID.
    pub fn get_by_player_id(&self, player_id: PlayerId) -> Option<Arc<PlayerInfo>> {
        self.players.get(&player_id).map(|r| Arc::clone(r.value()))
    }

    /// Get a PlayerInfo by client ID.
    pub fn get_by_client_id(&self, client_id: ClientId) -> Option<Arc<PlayerInfo>> {
        self.players_by_client
            .get(&client_id)
            .map(|r| Arc::clone(r.value()))
    }

    /// Get the next available player slot ID (0-14). Returns u8::MAX if full.
    pub fn next_available_player_id(&self) -> PlayerId {
        for i in 0u8..=14 {
            if !self.players.contains_key(&i) {
                return i;
            }
        }
        u8::MAX
    }

    /// Number of players.
    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    /// Return all player info entries.
    pub fn all_players(&self) -> Vec<Arc<PlayerInfo>> {
        self.players.iter().map(|r| Arc::clone(r.value())).collect()
    }

    /// Find a player by name (case-insensitive prefix match).
    pub fn find_player_by_name(&self, name: &str) -> Option<Arc<PlayerInfo>> {
        let lower = name.to_lowercase();
        self.players
            .iter()
            .find(|r| {
                let pn = r.player_name.lock();
                pn.to_lowercase().starts_with(&lower)
            })
            .map(|r| Arc::clone(r.value()))
    }

    /// Get all alive crewmates (non-impostor, non-dead).
    pub fn get_alive_crewmates(&self) -> Vec<Arc<PlayerInfo>> {
        self.players
            .iter()
            .filter(|r| {
                !r.is_impostor.load(Ordering::Relaxed)
                    && !r.is_dead.load(Ordering::Relaxed)
            })
            .map(|r| Arc::clone(r.value()))
            .collect()
    }

    /// Get all alive impostors.
    pub fn get_alive_impostors(&self) -> Vec<Arc<PlayerInfo>> {
        self.players
            .iter()
            .filter(|r| {
                r.is_impostor.load(Ordering::Relaxed)
                    && !r.is_dead.load(Ordering::Relaxed)
            })
            .map(|r| Arc::clone(r.value()))
            .collect()
    }

    /// Get all dead players.
    pub fn get_dead_players(&self) -> Vec<Arc<PlayerInfo>> {
        self.players
            .iter()
            .filter(|r| r.is_dead.load(Ordering::Relaxed))
            .map(|r| Arc::clone(r.value()))
            .collect()
    }

    /// Mark a player as dead.
    pub fn set_dead(&self, player_id: PlayerId) {
        if let Some(info) = self.players.get(&player_id) {
            info.is_dead.store(true, Ordering::SeqCst);
        }
    }

    /// Set a player's color.
    pub fn set_color(&self, player_id: PlayerId, color: u8) {
        if let Some(info) = self.players.get(&player_id) {
            *info.color.lock() = color;
        }
    }

    /// Set a player's name.
    pub fn set_name(&self, player_id: PlayerId, name: String) {
        if let Some(info) = self.players.get(&player_id) {
            *info.player_name.lock() = name;
        }
    }

    /// Set a player's hat.
    pub fn set_hat(&self, player_id: PlayerId, hat: String) {
        if let Some(info) = self.players.get(&player_id) {
            *info.hat.lock() = hat;
        }
    }

    /// Set a player's skin.
    pub fn set_skin(&self, player_id: PlayerId, skin: String) {
        if let Some(info) = self.players.get(&player_id) {
            *info.skin.lock() = skin;
        }
    }

    /// Set a player's pet.
    pub fn set_pet(&self, player_id: PlayerId, pet: String) {
        if let Some(info) = self.players.get(&player_id) {
            *info.pet.lock() = pet;
        }
    }

    /// Set a player's visor.
    pub fn set_visor(&self, player_id: PlayerId, visor: String) {
        if let Some(info) = self.players.get(&player_id) {
            *info.visor.lock() = visor;
        }
    }

    /// Set a player's nameplate.
    pub fn set_name_plate(&self, player_id: PlayerId, name_plate: String) {
        if let Some(info) = self.players.get(&player_id) {
            *info.name_plate.lock() = name_plate;
        }
    }

    /// Set a player's role.
    pub fn set_role(&self, player_id: PlayerId, role: RoleTypes) {
        if let Some(info) = self.players.get(&player_id) {
            *info.role.lock() = role;
            info.is_impostor.store(
                matches!(
                    role,
                    RoleTypes::Impostor | RoleTypes::Shapeshifter | RoleTypes::Phantom | RoleTypes::ImpostorGhost
                ),
                Ordering::SeqCst,
            );
        }
    }

    /// Set who is protecting this player.
    pub fn set_protection(&self, player_id: PlayerId, protector_id: PlayerId) {
        if let Some(info) = self.players.get(&player_id) {
            *info.protected_by.lock() = Some(protector_id);
        }
    }

    /// Clear protection for this player.
    pub fn clear_protection(&self, player_id: PlayerId) {
        if let Some(info) = self.players.get(&player_id) {
            *info.protected_by.lock() = None;
        }
    }

    /// Update task progress for a player.
    pub fn update_tasks(&self, player_id: PlayerId, completed: u32, total: u32) {
        if let Some(info) = self.players.get(&player_id) {
            info.tasks_completed.store(completed, Ordering::SeqCst);
            info.tasks_total.store(total, Ordering::SeqCst);
        }
    }

    /// Mark a player as impostor.
    pub fn mark_impostor(&self, player_id: PlayerId) {
        if let Some(info) = self.players.get(&player_id) {
            info.is_impostor.store(true, Ordering::SeqCst);
            let mut role = info.role.lock();
            if *role == RoleTypes::Crewmate {
                *role = RoleTypes::Impostor;
            }
        }
    }

    /// Get a list of all used colors (for CheckColor validation).
    pub fn used_colors(&self) -> Vec<u8> {
        self.players
            .iter()
            .map(|r| *r.color.lock())
            .collect()
    }

    /// Check if a name is already taken by another player.
    pub fn is_name_taken(&self, name: &str, exclude_client_id: ClientId) -> bool {
        let lower = name.to_lowercase();
        self.players.iter().any(|r| {
            r.client_id != exclude_client_id
                && r.player_name.lock().to_lowercase() == lower
        })
    }

    /// Check if a color is already used by another player.
    pub fn is_color_used(&self, color: u8, exclude_player_id: PlayerId) -> bool {
        self.players
            .iter()
            .any(|r| r.player_id != exclude_player_id && *r.color.lock() == color)
    }
}

/// Player session data: name, color, hat, skin, pet, etc.
///
/// Fields that change during gameplay use interior mutability (Mutex / Atomic)
/// so the struct can be shared via `Arc`.
pub struct PlayerInfo {
    pub net_id: NetId,
    pub owner_id: ClientId,
    pub spawn_flags: SpawnFlags,
    pub player_id: PlayerId,
    pub client_id: ClientId,

    // Mutable cosmetic / state fields
    pub player_name: Mutex<String>,
    pub color: Mutex<u8>,
    pub hat: Mutex<String>,
    pub skin: Mutex<String>,
    pub pet: Mutex<String>,
    pub visor: Mutex<String>,
    pub name_plate: Mutex<String>,

    pub is_impostor: AtomicBool,
    pub is_dead: AtomicBool,
    pub tasks_completed: AtomicU32,
    pub tasks_total: AtomicU32,

    pub controller_net_id: Option<NetId>,
    pub role: Mutex<RoleTypes>,
    pub protected_by: Mutex<Option<PlayerId>>,
    pub last_kill_time: Mutex<f64>,
}

impl PlayerInfo {
    pub fn new(player_id: PlayerId, client_id: ClientId) -> Self {
        Self {
            net_id: 0,
            owner_id: crate::SERVER_OWNED_ID,
            spawn_flags: SpawnFlags::NONE,
            player_id,
            client_id,
            player_name: Mutex::new(String::new()),
            color: Mutex::new(0),
            hat: Mutex::new(String::new()),
            skin: Mutex::new(String::new()),
            pet: Mutex::new(String::new()),
            visor: Mutex::new(String::new()),
            name_plate: Mutex::new(String::new()),
            is_impostor: AtomicBool::new(false),
            is_dead: AtomicBool::new(false),
            tasks_completed: AtomicU32::new(0),
            tasks_total: AtomicU32::new(0),
            controller_net_id: None,
            role: Mutex::new(RoleTypes::Crewmate),
            protected_by: Mutex::new(None),
            last_kill_time: Mutex::new(0.0),
        }
    }

    /// Serialize this PlayerInfo into a MessageWriter.
    pub fn serialize_into(&self, writer: &mut MessageWriter) {
        writer.write_byte(self.player_id);
        writer.write_packed_i32(self.client_id);

        // Name
        let name = self.player_name.lock();
        let name_str = name.clone();
        drop(name);
        let name_bytes = name_str.as_bytes();
        let name_len = name_bytes.len().min(255);
        writer.write_packed_u32(name_len as u32);
        writer.write_raw(&name_bytes[..name_len]);

        writer.write_byte(*self.color.lock());

        // Cosmetics
        for cosmetic in [
            self.hat.lock().clone(),
            self.skin.lock().clone(),
            self.pet.lock().clone(),
            self.visor.lock().clone(),
            self.name_plate.lock().clone(),
        ]
        .iter()
        {
            let bytes = cosmetic.as_bytes();
            let len = bytes.len().min(255);
            writer.write_packed_u32(len as u32);
            writer.write_raw(&bytes[..len]);
        }

        writer.write_bool(self.is_impostor.load(Ordering::Relaxed));
        writer.write_bool(self.is_dead.load(Ordering::Relaxed));
        writer.write_packed_u32(self.tasks_completed.load(Ordering::Relaxed));
        writer.write_packed_u32(self.tasks_total.load(Ordering::Relaxed));
        writer.write_packed_u32(*self.role.lock() as u16 as u32);
    }

    /// Deserialize PlayerInfo from a MessageReader.
    pub fn deserialize_from(reader: &mut MessageReader) -> Self {
        let player_id = reader.read_byte();
        let client_id = reader.read_packed_i32();
        let name = reader.read_string();
        let color = reader.read_byte();
        let hat = reader.read_string();
        let skin = reader.read_string();
        let pet = reader.read_string();
        let visor = reader.read_string();
        let name_plate = reader.read_string();
        let is_impostor = reader.read_bool();
        let is_dead = reader.read_bool();
        let tasks_completed = reader.read_packed_u32();
        let tasks_total = reader.read_packed_u32();
        let role_val = reader.read_packed_u32() as u16;
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
            _ => RoleTypes::Crewmate,
        };

        Self {
            net_id: 0,
            owner_id: crate::SERVER_OWNED_ID,
            spawn_flags: SpawnFlags::NONE,
            player_id,
            client_id,
            player_name: Mutex::new(name),
            color: Mutex::new(color),
            hat: Mutex::new(hat),
            skin: Mutex::new(skin),
            pet: Mutex::new(pet),
            visor: Mutex::new(visor),
            name_plate: Mutex::new(name_plate),
            is_impostor: AtomicBool::new(is_impostor),
            is_dead: AtomicBool::new(is_dead),
            tasks_completed: AtomicU32::new(tasks_completed),
            tasks_total: AtomicU32::new(tasks_total),
            controller_net_id: None,
            role: Mutex::new(role),
            protected_by: Mutex::new(None),
            last_kill_time: Mutex::new(0.0),
        }
    }
}

#[async_trait]
impl InnerNetObject for InnerGameData {
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

    async fn serialize(&self, writer: &mut MessageWriter, _initial: bool) -> GameResult<()> {
        let count = self.players.len() as u32;
        writer.write_packed_u32(count);

        for info in self.players.iter() {
            info.value().serialize_into(writer);
        }

        trace!("InnerGameData serialized: {} players", count);
        Ok(())
    }

    async fn deserialize(
        &mut self,
        _sender: &ClientPlayer,
        _target: Option<&ClientPlayer>,
        reader: &mut MessageReader,
        _initial: bool,
    ) -> GameResult<()> {
        let count = reader.read_packed_u32();

        self.players.clear();
        self.players_by_client.clear();

        for _ in 0..count {
            let info = PlayerInfo::deserialize_from(reader);
            let info = Arc::new(info);
            let pid = info.player_id;
            let cid = info.client_id;
            self.players.insert(pid, info.clone());
            self.players_by_client.insert(cid, info);
        }

        trace!("InnerGameData deserialized: {} players", count);
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
            // ---- Name RPCs ----
            RpcCalls::CheckName => {
                let name = reader.read_string();
                debug!(
                    "CheckName: client={} name=\"{}\"",
                    sender.client_id, name
                );

                // Validate name
                match anticheat::check_player_name(&name) {
                    anticheat::CheatResult::Cheat { message } => {
                        warn!("CheckName cheat: {}", message);
                        return Err(GameError::AntiCheatError(message));
                    }
                    anticheat::CheatResult::Allow => {}
                }

                // Check if name is taken by another player
                if self.is_name_taken(&name, sender.client_id) {
                    debug!("CheckName: name \"{}\" already taken", name);
                    return Ok(false);
                }

                Ok(true)
            }

            RpcCalls::SetName => {
                let name = reader.read_string();
                debug!(
                    "SetName: client={} name=\"{}\"",
                    sender.client_id, name
                );

                // Validate name
                match anticheat::check_player_name(&name) {
                    anticheat::CheatResult::Cheat { message } => {
                        warn!("SetName cheat: {}", message);
                        return Err(GameError::AntiCheatError(message));
                    }
                    anticheat::CheatResult::Allow => {}
                }

                // Update PlayerInfo
                if let Some(info) = self.get_by_client_id(sender.client_id) {
                    self.set_name(info.player_id, name.clone());
                }

                Ok(true)
            }

            // ---- Color RPCs ----
            RpcCalls::CheckColor => {
                let color = reader.read_byte();
                debug!(
                    "CheckColor: client={} color={}",
                    sender.client_id, color
                );

                // Validate color range (0-17 are valid Among Us colors)
                if color > 17 {
                    warn!("CheckColor: color {} out of range", color);
                    return Err(GameError::CheatDetected {
                        message: format!("color {} out of valid range 0-17", color),
                    });
                }

                // Find sender's player info to exclude self from check
                let sender_pid = self
                    .get_by_client_id(sender.client_id)
                    .map(|i| i.player_id)
                    .unwrap_or(u8::MAX);

                if self.is_color_used(color, sender_pid) {
                    debug!("CheckColor: color {} already used", color);
                    return Ok(false);
                }

                Ok(true)
            }

            RpcCalls::SetColor => {
                let color = reader.read_byte();
                debug!(
                    "SetColor: client={} color={}",
                    sender.client_id, color
                );

                if color > 17 {
                    warn!("SetColor: color {} out of range", color);
                    return Err(GameError::CheatDetected {
                        message: format!("color {} out of valid range 0-17", color),
                    });
                }

                if let Some(info) = self.get_by_client_id(sender.client_id) {
                    self.set_color(info.player_id, color);
                }

                Ok(true)
            }

            // ---- Cosmetic RPCs (string variants) ----
            RpcCalls::SetHatStr => {
                let hat = reader.read_string();
                debug!("SetHatStr: client={} hat=\"{}\"", sender.client_id, hat);

                if let Some(info) = self.get_by_client_id(sender.client_id) {
                    self.set_hat(info.player_id, hat);
                }
                Ok(true)
            }

            RpcCalls::SetSkinStr => {
                let skin = reader.read_string();
                debug!(
                    "SetSkinStr: client={} skin=\"{}\"",
                    sender.client_id, skin
                );

                if let Some(info) = self.get_by_client_id(sender.client_id) {
                    self.set_skin(info.player_id, skin);
                }
                Ok(true)
            }

            RpcCalls::SetPetStr => {
                let pet = reader.read_string();
                debug!("SetPetStr: client={} pet=\"{}\"", sender.client_id, pet);

                if let Some(info) = self.get_by_client_id(sender.client_id) {
                    self.set_pet(info.player_id, pet);
                }
                Ok(true)
            }

            RpcCalls::SetVisorStr => {
                let visor = reader.read_string();
                debug!(
                    "SetVisorStr: client={} visor=\"{}\"",
                    sender.client_id, visor
                );

                if let Some(info) = self.get_by_client_id(sender.client_id) {
                    self.set_visor(info.player_id, visor);
                }
                Ok(true)
            }

            RpcCalls::SetNamePlateStr => {
                let name_plate = reader.read_string();
                debug!(
                    "SetNamePlateStr: client={} nameplate=\"{}\"",
                    sender.client_id, name_plate
                );

                if let Some(info) = self.get_by_client_id(sender.client_id) {
                    self.set_name_plate(info.player_id, name_plate);
                }
                Ok(true)
            }

            // ---- Cosmetic RPCs (legacy integer variants) ----
            RpcCalls::SetHat => {
                let hat_id = reader.read_packed_u32();
                let hat = hat_id.to_string();
                debug!(
                    "SetHat: client={} hat_id={}",
                    sender.client_id, hat_id
                );

                if let Some(info) = self.get_by_client_id(sender.client_id) {
                    self.set_hat(info.player_id, hat);
                }
                Ok(true)
            }

            RpcCalls::SetSkin => {
                let skin_id = reader.read_packed_u32();
                let skin = skin_id.to_string();
                debug!(
                    "SetSkin: client={} skin_id={}",
                    sender.client_id, skin_id
                );

                if let Some(info) = self.get_by_client_id(sender.client_id) {
                    self.set_skin(info.player_id, skin);
                }
                Ok(true)
            }

            RpcCalls::SetPet => {
                let pet_id = reader.read_packed_u32();
                let pet = pet_id.to_string();
                debug!(
                    "SetPet: client={} pet_id={}",
                    sender.client_id, pet_id
                );

                if let Some(info) = self.get_by_client_id(sender.client_id) {
                    self.set_pet(info.player_id, pet);
                }
                Ok(true)
            }

            RpcCalls::SetVisor => {
                let visor_id = reader.read_packed_u32();
                let visor = visor_id.to_string();
                debug!(
                    "SetVisor: client={} visor_id={}",
                    sender.client_id, visor_id
                );

                if let Some(info) = self.get_by_client_id(sender.client_id) {
                    self.set_visor(info.player_id, visor);
                }
                Ok(true)
            }

            RpcCalls::SetNamePlate => {
                let nameplate_id = reader.read_packed_u32();
                let nameplate = nameplate_id.to_string();
                debug!(
                    "SetNamePlate: client={} nameplate_id={}",
                    sender.client_id, nameplate_id
                );

                if let Some(info) = self.get_by_client_id(sender.client_id) {
                    self.set_name_plate(info.player_id, nameplate);
                }
                Ok(true)
            }

            // ---- Role RPCs ----
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

                // Host-only operation check
                if !sender.is_host {
                    return Err(GameError::HostOnlyOperation(sender.client_id));
                }

                if let Some(info) = self.get_by_client_id(sender.client_id) {
                    self.set_role(info.player_id, role);
                }
                Ok(true)
            }

            // ---- SetInfected: mark impostors ----
            RpcCalls::SetInfected => {
                if reader.is_empty() {
                    return Ok(true);
                }

                let count = reader.read_packed_u32();
                debug!(
                    "SetInfected: setting {} impostor(s)",
                    count
                );

                let mut impostor_ids: Vec<PlayerId> = Vec::new();
                for _ in 0..count {
                    let pid = reader.read_byte();
                    impostor_ids.push(pid);
                }

                for pid in &impostor_ids {
                    self.mark_impostor(*pid);
                    self.set_role(*pid, RoleTypes::Impostor);
                }

                // Also mark the local sender's player if they are in the list
                if let Some(info) = self.get_by_client_id(sender.client_id) {
                    if impostor_ids.contains(&info.player_id) {
                        self.set_role(info.player_id, RoleTypes::Impostor);
                    }
                }

                Ok(true)
            }

            // ---- Unhandled RPCs for this object ----
            _ => {
                trace!(
                    "InnerGameData: unhandled RPC {:?}",
                    call
                );
                Ok(false)
            }
        }
    }
}
