//! InnerShipStatus — base ship status for all map types.
//!
//! Manages system states (reactor, electrical, O2, doors, etc.),
//! sabotage tracking, and serialization/deserialization of the
//! ship's networked state.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use railway_hazel::{MessageReader, MessageWriter};
use railway_protocol::{ClientId, MapType, NetId, PlayerId, RpcCalls, SpawnFlags};
use parking_lot::{Mutex, RwLock};
use tracing::debug;

use crate::objects::InnerNetObject;
use crate::player::ClientPlayer;
use crate::{Game, GameError, GameResult};

// ── SystemTypes enum ──────────────────────────────────────────────

/// All system types present across the various Among Us maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SystemTypes {
    Reactor = 0,
    Electrical = 1,
    O2 = 2,
    MedBay = 3,
    Security = 4,
    Sabotage = 5,
    Doors = 6,
    Comms = 7,
    Laboratory = 8,
    LifeSupport = 9,
    HeliSabotage = 10,
    MushroomMixup = 11,
    Decontamination = 12,
    MovingPlatform = 13,
    ElectricalDoors = 14,
    AutoDoors = 15,
    HudOverride = 16,
}

impl SystemTypes {
    /// Convert a u8 to a SystemTypes variant, returning None for unknown values.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Reactor),
            1 => Some(Self::Electrical),
            2 => Some(Self::O2),
            3 => Some(Self::MedBay),
            4 => Some(Self::Security),
            5 => Some(Self::Sabotage),
            6 => Some(Self::Doors),
            7 => Some(Self::Comms),
            8 => Some(Self::Laboratory),
            9 => Some(Self::LifeSupport),
            10 => Some(Self::HeliSabotage),
            11 => Some(Self::MushroomMixup),
            12 => Some(Self::Decontamination),
            13 => Some(Self::MovingPlatform),
            14 => Some(Self::ElectricalDoors),
            15 => Some(Self::AutoDoors),
            16 => Some(Self::HudOverride),
            _ => None,
        }
    }

    /// Returns a human-readable name for this system type.
    pub fn name(self) -> &'static str {
        match self {
            Self::Reactor => "Reactor",
            Self::Electrical => "Electrical",
            Self::O2 => "O2",
            Self::MedBay => "MedBay",
            Self::Security => "Security",
            Self::Sabotage => "Sabotage",
            Self::Doors => "Doors",
            Self::Comms => "Comms",
            Self::Laboratory => "Laboratory",
            Self::LifeSupport => "LifeSupport",
            Self::HeliSabotage => "HeliSabotage",
            Self::MushroomMixup => "MushroomMixup",
            Self::Decontamination => "Decontamination",
            Self::MovingPlatform => "MovingPlatform",
            Self::ElectricalDoors => "ElectricalDoors",
            Self::AutoDoors => "AutoDoors",
            Self::HudOverride => "HudOverride",
        }
    }
}

// ── ShipSystem enum ───────────────────────────────────────────────

/// State for a single ship system. Each variant holds data specific
/// to that system type.
#[derive(Debug, Clone)]
pub enum ShipSystem {
    /// Reactor meltdown sabotage.
    Reactor {
        countdown: f32,
        user_consoles: Vec<u8>,
    },
    /// Electrical switches (calibrate distributor, etc.).
    SwitchSystem {
        expected_value: u8,
        current_value: u8,
    },
    /// Manual doors that can be opened/closed by the impostor.
    DoorsSystem {
        door_ids: Vec<u8>,
        is_open: bool,
        timer: f32,
    },
    /// Generic sabotage system (comms, etc.).
    SabotageSystem {
        is_active: bool,
        timer: f32,
    },
    /// Security cameras.
    SecurityCameraSystem {
        is_active: bool,
        player_count: u8,
    },
    /// MedBay scan.
    MedScanSystem {
        player_ids: Vec<PlayerId>,
        scan_queue: Vec<PlayerId>,
    },
    /// O2 / Life Support sabotage.
    LifeSuppSystem {
        countdown: f32,
        completed_consoles: u8,
    },
    /// Airship helicopter sabotage.
    HeliSabotageSystem {
        countdown: f32,
        is_active: bool,
    },
    /// HUD override (disables task list, sabotages UI).
    HudOverrideSystem {
        is_active: bool,
        timer: f32,
    },
    /// Electrical doors (specific door systems that are electrically controlled).
    ElectricalDoors {
        door_ids: Vec<u8>,
        is_closed: bool,
    },
    /// Automatic doors (Polus).
    AutoDoorsSystem {
        door_ids: Vec<u8>,
        auto_close_timer: f32,
    },
    /// Airship moving platform.
    MovingPlatformBehaviour {
        position: f32,
        is_moving: bool,
        target: f32,
    },
    /// Fungle mushroom mixup sabotage.
    MushroomMixupSabotageSystem {
        is_active: bool,
        shuffle_timer: f32,
    },
}

impl ShipSystem {
    /// Returns the system type discriminant for this variant.
    pub fn system_type(&self) -> u8 {
        match self {
            Self::Reactor { .. } => SystemTypes::Reactor as u8,
            Self::SwitchSystem { .. } => SystemTypes::Electrical as u8,
            Self::DoorsSystem { .. } => SystemTypes::Doors as u8,
            Self::SabotageSystem { .. } => SystemTypes::Sabotage as u8,
            Self::SecurityCameraSystem { .. } => SystemTypes::Security as u8,
            Self::MedScanSystem { .. } => SystemTypes::MedBay as u8,
            Self::LifeSuppSystem { .. } => SystemTypes::LifeSupport as u8,
            Self::HeliSabotageSystem { .. } => SystemTypes::HeliSabotage as u8,
            Self::HudOverrideSystem { .. } => SystemTypes::HudOverride as u8,
            Self::ElectricalDoors { .. } => SystemTypes::ElectricalDoors as u8,
            Self::AutoDoorsSystem { .. } => SystemTypes::AutoDoors as u8,
            Self::MovingPlatformBehaviour { .. } => SystemTypes::MovingPlatform as u8,
            Self::MushroomMixupSabotageSystem { .. } => SystemTypes::MushroomMixup as u8,
        }
    }

    /// Returns true if this system represents an active sabotage.
    ///
    /// Matches C#'s `SabotageSystemType.IsActive`: Reactor/LifeSupp/HeliSabotage
    /// use `10000f` as the "inactive" sentinel — active means `countdown < 10000`.
    pub fn is_sabotage(&self) -> bool {
        const INACTIVE_SENTINEL: f32 = 10000.0;
        match self {
            Self::Reactor { countdown, .. } => *countdown < INACTIVE_SENTINEL && *countdown > 0.0,
            Self::SabotageSystem { is_active, .. } => *is_active,
            Self::LifeSuppSystem { countdown, .. } => *countdown < INACTIVE_SENTINEL && *countdown > 0.0,
            Self::HeliSabotageSystem { is_active, .. } => *is_active,
            Self::HudOverrideSystem { is_active, .. } => *is_active,
            Self::MushroomMixupSabotageSystem { is_active, .. } => *is_active,
            _ => false,
        }
    }

    /// Serialize this system's state into the writer.
    pub fn serialize_into(&self, writer: &mut MessageWriter) {
        match self {
            Self::Reactor { countdown, user_consoles } => {
                writer.write_f32(*countdown);
                writer.write_packed_u32(user_consoles.len() as u32);
                for console in user_consoles {
                    writer.write_byte(*console);
                }
            }
            Self::SwitchSystem { expected_value, current_value } => {
                writer.write_byte(*expected_value);
                writer.write_byte(*current_value);
            }
            Self::DoorsSystem { door_ids, is_open, timer } => {
                writer.write_packed_u32(door_ids.len() as u32);
                for id in door_ids {
                    writer.write_byte(*id);
                }
                writer.write_bool(*is_open);
                writer.write_f32(*timer);
            }
            Self::SabotageSystem { is_active, timer } => {
                writer.write_bool(*is_active);
                writer.write_f32(*timer);
            }
            Self::SecurityCameraSystem { is_active, player_count } => {
                writer.write_bool(*is_active);
                writer.write_byte(*player_count);
            }
            Self::MedScanSystem { player_ids, scan_queue } => {
                writer.write_packed_u32(player_ids.len() as u32);
                for pid in player_ids {
                    writer.write_byte(*pid);
                }
                writer.write_packed_u32(scan_queue.len() as u32);
                for pid in scan_queue {
                    writer.write_byte(*pid);
                }
            }
            Self::LifeSuppSystem { countdown, completed_consoles } => {
                writer.write_f32(*countdown);
                writer.write_byte(*completed_consoles);
            }
            Self::HeliSabotageSystem { countdown, is_active } => {
                writer.write_f32(*countdown);
                writer.write_bool(*is_active);
            }
            Self::HudOverrideSystem { is_active, timer } => {
                writer.write_bool(*is_active);
                writer.write_f32(*timer);
            }
            Self::ElectricalDoors { door_ids, is_closed } => {
                writer.write_packed_u32(door_ids.len() as u32);
                for id in door_ids {
                    writer.write_byte(*id);
                }
                writer.write_bool(*is_closed);
            }
            Self::AutoDoorsSystem { door_ids, auto_close_timer } => {
                writer.write_packed_u32(door_ids.len() as u32);
                for id in door_ids {
                    writer.write_byte(*id);
                }
                writer.write_f32(*auto_close_timer);
            }
            Self::MovingPlatformBehaviour { position, is_moving, target } => {
                writer.write_f32(*position);
                writer.write_bool(*is_moving);
                writer.write_f32(*target);
            }
            Self::MushroomMixupSabotageSystem { is_active, shuffle_timer } => {
                writer.write_bool(*is_active);
                writer.write_f32(*shuffle_timer);
            }
        }
    }

    /// Deserialize system state from the reader, returning a new ShipSystem.
    pub fn deserialize_from(system_type: u8, reader: &mut MessageReader) -> Option<Self> {
        match system_type {
            0 => {
                let countdown = reader.read_f32();
                let count = reader.read_packed_u32() as usize;
                let mut user_consoles = Vec::with_capacity(count);
                for _ in 0..count {
                    user_consoles.push(reader.read_byte());
                }
                Some(Self::Reactor { countdown, user_consoles })
            }
            1 => {
                let expected_value = reader.read_byte();
                let current_value = reader.read_byte();
                Some(Self::SwitchSystem { expected_value, current_value })
            }
            6 => {
                let count = reader.read_packed_u32() as usize;
                let mut door_ids = Vec::with_capacity(count);
                for _ in 0..count {
                    door_ids.push(reader.read_byte());
                }
                let is_open = reader.read_bool();
                let timer = reader.read_f32();
                Some(Self::DoorsSystem { door_ids, is_open, timer })
            }
            5 => {
                let is_active = reader.read_bool();
                let timer = reader.read_f32();
                Some(Self::SabotageSystem { is_active, timer })
            }
            4 => {
                let is_active = reader.read_bool();
                let player_count = reader.read_byte();
                Some(Self::SecurityCameraSystem { is_active, player_count })
            }
            3 => {
                let count = reader.read_packed_u32() as usize;
                let mut player_ids = Vec::with_capacity(count);
                for _ in 0..count {
                    player_ids.push(reader.read_byte());
                }
                let qcount = reader.read_packed_u32() as usize;
                let mut scan_queue = Vec::with_capacity(qcount);
                for _ in 0..qcount {
                    scan_queue.push(reader.read_byte());
                }
                Some(Self::MedScanSystem { player_ids, scan_queue })
            }
            9 => {
                let countdown = reader.read_f32();
                let completed_consoles = reader.read_byte();
                Some(Self::LifeSuppSystem { countdown, completed_consoles })
            }
            10 => {
                let countdown = reader.read_f32();
                let is_active = reader.read_bool();
                Some(Self::HeliSabotageSystem { countdown, is_active })
            }
            16 => {
                let is_active = reader.read_bool();
                let timer = reader.read_f32();
                Some(Self::HudOverrideSystem { is_active, timer })
            }
            14 => {
                let count = reader.read_packed_u32() as usize;
                let mut door_ids = Vec::with_capacity(count);
                for _ in 0..count {
                    door_ids.push(reader.read_byte());
                }
                let is_closed = reader.read_bool();
                Some(Self::ElectricalDoors { door_ids, is_closed })
            }
            15 => {
                let count = reader.read_packed_u32() as usize;
                let mut door_ids = Vec::with_capacity(count);
                for _ in 0..count {
                    door_ids.push(reader.read_byte());
                }
                let auto_close_timer = reader.read_f32();
                Some(Self::AutoDoorsSystem { door_ids, auto_close_timer })
            }
            13 => {
                let position = reader.read_f32();
                let is_moving = reader.read_bool();
                let target = reader.read_f32();
                Some(Self::MovingPlatformBehaviour { position, is_moving, target })
            }
            11 => {
                let is_active = reader.read_bool();
                let shuffle_timer = reader.read_f32();
                Some(Self::MushroomMixupSabotageSystem { is_active, shuffle_timer })
            }
            _ => None,
        }
    }
}

// ── InnerShipStatus ───────────────────────────────────────────────

/// Base ship status object managing all system states for a map.
///
/// Each map-specific ship status (Skeld, Mira, Polus, etc.) wraps
/// this struct and provides the list of systems present on that map.
pub struct InnerShipStatus {
    net_id: NetId,
    owner_id: ClientId,
    spawn_flags: SpawnFlags,
    /// Reference to the game this ship status belongs to.
    pub game: Arc<Game>,
    /// The map type this ship status is for.
    pub map_type: Mutex<MapType>,
    /// Map of system type -> system state.
    pub systems: DashMap<u8, ShipSystem>,
    /// Whether any sabotage is currently active.
    pub sabotage_active: RwLock<bool>,
    /// Set of door system types that are currently closed.
    pub doors_closed: Mutex<HashSet<u8>>,
    /// Tracks how many emergency meetings have been called.
    pub emergency_count: Mutex<u32>,
}

impl InnerShipStatus {
    /// Create a new ship status for the given map type.
    pub fn new(game: Arc<Game>, map_type: MapType) -> Self {
        let mut status = Self {
            net_id: 0,
            owner_id: crate::SERVER_OWNED_ID,
            spawn_flags: SpawnFlags::NONE,
            game,
            map_type: Mutex::new(map_type),
            systems: DashMap::new(),
            sabotage_active: RwLock::new(false),
            doors_closed: Mutex::new(HashSet::new()),
            emergency_count: Mutex::new(0),
        };
        status.initialize_default_systems(map_type);
        status
    }

    /// Initialize default system states for the given map type.
    fn initialize_default_systems(&mut self, map_type: MapType) {
        let system_types = Self::systems_for_map(map_type);
        for sys_type in system_types {
            let system = Self::default_system_for_type(*sys_type);
            self.systems.insert(*sys_type as u8, system);
        }
    }

    /// Return the list of system types present on a given map.
    pub fn systems_for_map(map_type: MapType) -> &'static [SystemTypes] {
        match map_type {
            MapType::Skeld => &[
                SystemTypes::Reactor,
                SystemTypes::Electrical,
                SystemTypes::O2,
                SystemTypes::MedBay,
                SystemTypes::Security,
                SystemTypes::Sabotage,
                SystemTypes::Doors,
                SystemTypes::Comms,
            ],
            MapType::MiraHQ => &[
                SystemTypes::Reactor,
                SystemTypes::Electrical,
                SystemTypes::O2,
                SystemTypes::MedBay,
                SystemTypes::Doors,
                SystemTypes::Comms,
                SystemTypes::Decontamination,
            ],
            MapType::Polus => &[
                SystemTypes::Reactor,
                SystemTypes::Electrical,
                SystemTypes::O2,
                SystemTypes::Security,
                SystemTypes::Doors,
                SystemTypes::Comms,
                SystemTypes::Laboratory,
                SystemTypes::AutoDoors,
            ],
            MapType::Dleks => &[
                SystemTypes::Reactor,
                SystemTypes::Electrical,
                SystemTypes::O2,
                SystemTypes::MedBay,
                SystemTypes::Security,
                SystemTypes::Sabotage,
                SystemTypes::Doors,
                SystemTypes::Comms,
            ],
            MapType::Airship => &[
                SystemTypes::Reactor,
                SystemTypes::Electrical,
                SystemTypes::Security,
                SystemTypes::Doors,
                SystemTypes::Comms,
                SystemTypes::MovingPlatform,
                SystemTypes::HeliSabotage,
            ],
            MapType::Fungle => &[
                SystemTypes::Reactor,
                SystemTypes::Electrical,
                SystemTypes::O2,
                SystemTypes::Security,
                SystemTypes::Doors,
                SystemTypes::Comms,
                SystemTypes::Laboratory,
                SystemTypes::MushroomMixup,
                SystemTypes::HudOverride,
            ],
        }
    }

    /// Create a default system state for a given system type.
    fn default_system_for_type(sys_type: SystemTypes) -> ShipSystem {
        match sys_type {
            SystemTypes::Reactor => ShipSystem::Reactor {
                countdown: 0.0,
                user_consoles: vec![0, 1],
            },
            SystemTypes::Electrical => ShipSystem::SwitchSystem {
                expected_value: 0,
                current_value: 0,
            },
            SystemTypes::O2 => ShipSystem::LifeSuppSystem {
                countdown: 0.0,
                completed_consoles: 0,
            },
            SystemTypes::MedBay => ShipSystem::MedScanSystem {
                player_ids: Vec::new(),
                scan_queue: Vec::new(),
            },
            SystemTypes::Security => ShipSystem::SecurityCameraSystem {
                is_active: false,
                player_count: 0,
            },
            SystemTypes::Sabotage => ShipSystem::SabotageSystem {
                is_active: false,
                timer: 0.0,
            },
            SystemTypes::Doors => ShipSystem::DoorsSystem {
                door_ids: (0u8..13).collect(),
                is_open: true,
                timer: 0.0,
            },
            SystemTypes::Comms => ShipSystem::SabotageSystem {
                is_active: false,
                timer: 0.0,
            },
            SystemTypes::Laboratory => ShipSystem::SabotageSystem {
                is_active: false,
                timer: 0.0,
            },
            SystemTypes::LifeSupport => ShipSystem::LifeSuppSystem {
                countdown: 0.0,
                completed_consoles: 0,
            },
            SystemTypes::HeliSabotage => ShipSystem::HeliSabotageSystem {
                countdown: 0.0,
                is_active: false,
            },
            SystemTypes::HudOverride => ShipSystem::HudOverrideSystem {
                is_active: false,
                timer: 0.0,
            },
            SystemTypes::MushroomMixup => ShipSystem::MushroomMixupSabotageSystem {
                is_active: false,
                shuffle_timer: 0.0,
            },
            SystemTypes::Decontamination => ShipSystem::DoorsSystem {
                door_ids: vec![0, 1, 2],
                is_open: true,
                timer: 0.0,
            },
            SystemTypes::MovingPlatform => ShipSystem::MovingPlatformBehaviour {
                position: 0.0,
                is_moving: false,
                target: 0.0,
            },
            SystemTypes::ElectricalDoors => ShipSystem::ElectricalDoors {
                door_ids: Vec::new(),
                is_closed: false,
            },
            SystemTypes::AutoDoors => ShipSystem::AutoDoorsSystem {
                door_ids: Vec::new(),
                auto_close_timer: 0.0,
            },
        }
    }

    /// Update a system — applies repair or sabotage progress.
    ///
    /// `amount` represents the repair/sabotage amount. For repairs,
    /// positive amounts move toward completion; for sabotage, the
    /// significance depends on the system type.
    ///
    /// Returns an error if the system type is not present on this map.
    pub fn update_system(
        &self,
        system_type: u8,
        amount: u8,
        player_id: PlayerId,
    ) -> GameResult<()> {
        let mut system = self.systems.get_mut(&system_type).ok_or_else(|| {
            GameError::GameLogic {
                code: self.game.code,
                message: format!(
                    "system type {} not found on map {:?}",
                    system_type,
                    *self.map_type.lock()
                ),
            }
        })?;

        let sys = system.value_mut();

        match sys {
            ShipSystem::Reactor { countdown, user_consoles: _ } => {
                // Positive amount = repair progress, reduces countdown
                let repair_amount = amount as f32 * 0.1;
                *countdown = (*countdown - repair_amount).max(0.0);

                // If repaired, clear the sabotage
                if *countdown <= 0.0 {
                    *countdown = 0.0;
                    *self.sabotage_active.write() = self
                        .systems
                        .iter()
                        .any(|s| s.value().is_sabotage());
                    debug!(
                        "game {}: reactor repaired by player {}",
                        self.game.code, player_id
                    );
                }
            }
            ShipSystem::SwitchSystem { expected_value, current_value } => {
                // Each repair tick moves current toward expected
                if *current_value < *expected_value {
                    *current_value = (*current_value + amount).min(*expected_value);
                }
                debug!(
                    "game {}: switch system updated: {}/{} by player {}",
                    self.game.code, *current_value, *expected_value, player_id
                );
            }
            ShipSystem::LifeSuppSystem { countdown, completed_consoles } => {
                let repair_amount = amount as f32 * 0.1;
                *countdown = (*countdown - repair_amount).max(0.0);
                if *countdown <= 0.0 {
                    *countdown = 0.0;
                    *completed_consoles += 1;
                    *self.sabotage_active.write() = self
                        .systems
                        .iter()
                        .any(|s| s.value().is_sabotage());
                    debug!(
                        "game {}: life support repaired, consoles: {}",
                        self.game.code, *completed_consoles
                    );
                }
            }
            ShipSystem::HeliSabotageSystem { countdown, is_active } => {
                let repair_amount = amount as f32 * 0.1;
                *countdown = (*countdown - repair_amount).max(0.0);
                if *countdown <= 0.0 {
                    *countdown = 0.0;
                    *is_active = false;
                    *self.sabotage_active.write() = self
                        .systems
                        .iter()
                        .any(|s| s.value().is_sabotage());
                    debug!("game {}: heli sabotage repaired by player {}", self.game.code, player_id);
                }
            }
            ShipSystem::HudOverrideSystem { is_active, timer } => {
                let repair_amount = amount as f32 * 0.1;
                *timer = (*timer - repair_amount).max(0.0);
                if *timer <= 0.0 {
                    *is_active = false;
                    *timer = 0.0;
                    *self.sabotage_active.write() = self
                        .systems
                        .iter()
                        .any(|s| s.value().is_sabotage());
                    debug!("game {}: HUD override repaired by player {}", self.game.code, player_id);
                }
            }
            ShipSystem::MushroomMixupSabotageSystem { is_active, shuffle_timer } => {
                let repair_amount = amount as f32 * 0.1;
                *shuffle_timer = (*shuffle_timer - repair_amount).max(0.0);
                if *shuffle_timer <= 0.0 {
                    *is_active = false;
                    *shuffle_timer = 0.0;
                    *self.sabotage_active.write() = self
                        .systems
                        .iter()
                        .any(|s| s.value().is_sabotage());
                    debug!("game {}: mushroom mixup repaired by player {}", self.game.code, player_id);
                }
            }
            ShipSystem::SabotageSystem { is_active, timer } => {
                let repair_amount = amount as f32 * 0.1;
                *timer = (*timer - repair_amount).max(0.0);
                if *timer <= 0.0 && *is_active {
                    *is_active = false;
                    *timer = 0.0;
                    *self.sabotage_active.write() = self
                        .systems
                        .iter()
                        .any(|s| s.value().is_sabotage());
                    debug!(
                        "game {}: sabotage system {} repaired by player {}",
                        self.game.code, system_type, player_id
                    );
                }
            }
            ShipSystem::MedScanSystem { player_ids, scan_queue } => {
                // Adding player to scan
                if !player_ids.contains(&player_id) {
                    player_ids.push(player_id);
                }
                scan_queue.retain(|&id| id != player_id);
                debug!("game {}: player {} added to med scan", self.game.code, player_id);
            }
            // For door systems, update_system can be used to open/close
            ShipSystem::DoorsSystem { is_open, timer, .. } => {
                *is_open = amount > 0;
                if !*is_open {
                    *timer = 10.0; // auto-close timer
                    let mut doors = self.doors_closed.lock();
                    doors.insert(system_type);
                } else {
                    let mut doors = self.doors_closed.lock();
                    doors.remove(&system_type);
                }
                debug!(
                    "game {}: doors system {} {}",
                    self.game.code,
                    system_type,
                    if *is_open { "opened" } else { "closed" }
                );
            }
            ShipSystem::ElectricalDoors { is_closed, .. } => {
                *is_closed = amount > 0;
                if *is_closed {
                    let mut doors = self.doors_closed.lock();
                    doors.insert(system_type);
                } else {
                    let mut doors = self.doors_closed.lock();
                    doors.remove(&system_type);
                }
            }
            ShipSystem::AutoDoorsSystem { auto_close_timer, .. } => {
                *auto_close_timer = if amount > 0 { 5.0 } else { 0.0 };
            }
            ShipSystem::MovingPlatformBehaviour { position: _, is_moving, target } => {
                *target = amount as f32;
                *is_moving = true;
                debug!(
                    "game {}: moving platform target set to {} by player {}",
                    self.game.code, *target, player_id
                );
            }
            ShipSystem::SecurityCameraSystem { is_active, player_count } => {
                *is_active = amount > 0;
                if *is_active {
                    *player_count = player_count.saturating_add(1);
                } else {
                    *player_count = player_count.saturating_sub(1);
                }
            }
        }

        Ok(())
    }

    /// Close all doors of a given system type (impostor sabotage action).
    pub fn close_doors_of_type(&self, system_type: u8) -> GameResult<()> {
        let mut system = self.systems.get_mut(&system_type).ok_or_else(|| {
            GameError::GameLogic {
                code: self.game.code,
                message: format!("cannot close doors: system type {} not found", system_type),
            }
        })?;

        let sys = system.value_mut();

        match sys {
            ShipSystem::DoorsSystem { is_open, timer, .. } => {
                *is_open = false;
                *timer = 10.0;
                let mut doors = self.doors_closed.lock();
                doors.insert(system_type);
                debug!("game {}: doors of type {} closed", self.game.code, system_type);
            }
            ShipSystem::ElectricalDoors { is_closed, .. } => {
                *is_closed = true;
                let mut doors = self.doors_closed.lock();
                doors.insert(system_type);
                debug!("game {}: electrical doors {} closed", self.game.code, system_type);
            }
            ShipSystem::AutoDoorsSystem { auto_close_timer, .. } => {
                *auto_close_timer = 5.0;
                let mut doors = self.doors_closed.lock();
                doors.insert(system_type);
                debug!("game {}: auto doors {} triggered", self.game.code, system_type);
            }
            _ => {
                return Err(GameError::GameLogic {
                    code: self.game.code,
                    message: format!("system type {} is not a door system", system_type),
                });
            }
        }

        Ok(())
    }

    /// Get a reference to a system by its type.
    pub fn get_system(&self, system_type: u8) -> Option<dashmap::mapref::one::Ref<'_, u8, ShipSystem>> {
        self.systems.get(&system_type)
    }

    /// Returns true if any active sabotage is in progress.
    pub fn is_sabotage_active(&self) -> bool {
        *self.sabotage_active.read()
    }

    /// Repair a sabotaged system.
    pub fn repair_system(&self, system_type: u8, player_id: PlayerId) -> GameResult<()> {
        // A repair is just an update with a full repair amount
        self.update_system(system_type, 10, player_id)?;

        debug!(
            "game {}: system type {} repaired by player {}",
            self.game.code, system_type, player_id
        );

        Ok(())
    }

    /// Trigger a sabotage on a system (called by impostor).
    pub fn trigger_sabotage(&self, system_type: u8) -> GameResult<()> {
        let mut system = self.systems.get_mut(&system_type).ok_or_else(|| {
            GameError::GameLogic {
                code: self.game.code,
                message: format!("cannot sabotage: system type {} not found", system_type),
            }
        })?;

        let sys = system.value_mut();

        match sys {
            ShipSystem::Reactor { countdown, .. } => {
                *countdown = 45.0;
                *self.sabotage_active.write() = true;
            }
            ShipSystem::LifeSuppSystem { countdown, .. } => {
                *countdown = 45.0;
                *self.sabotage_active.write() = true;
            }
            ShipSystem::SabotageSystem { is_active, timer } => {
                *is_active = true;
                *timer = 30.0;
                *self.sabotage_active.write() = true;
            }
            ShipSystem::HeliSabotageSystem { countdown, is_active } => {
                *countdown = 45.0;
                *is_active = true;
                *self.sabotage_active.write() = true;
            }
            ShipSystem::HudOverrideSystem { is_active, timer } => {
                *is_active = true;
                *timer = 15.0;
                *self.sabotage_active.write() = true;
            }
            ShipSystem::MushroomMixupSabotageSystem { is_active, shuffle_timer } => {
                *is_active = true;
                *shuffle_timer = 20.0;
                *self.sabotage_active.write() = true;
            }
            _ => {
                return Err(GameError::GameLogic {
                    code: self.game.code,
                    message: format!("system type {} cannot be sabotaged", system_type),
                });
            }
        }

        debug!("game {}: sabotage triggered on system {}", self.game.code, system_type);
        Ok(())
    }

    /// Check if the sabotage timer for a critical system has expired.
    /// If so, the game should end (impostors win by sabotage).
    pub fn check_sabotage_timeout(&self) -> bool {
        for entry in self.systems.iter() {
            match entry.value() {
                ShipSystem::Reactor { countdown, .. } if *countdown <= 0.0 && *countdown == 0.0 => {
                    // Reactor meltdown complete — but only if it was active
                }
                ShipSystem::LifeSuppSystem { countdown, .. } if *countdown <= 0.0 => {
                    // O2 depleted
                }
                _ => {}
            }
        }
        false
    }
}

// ── InnerNetObject impl ───────────────────────────────────────────

#[async_trait]
impl InnerNetObject for InnerShipStatus {
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

    async fn serialize(&self, writer: &mut MessageWriter, _initial_state: bool) -> GameResult<()> {
        let map_type = *self.map_type.lock();
        writer.write_byte(map_type as u8);

        // Write system count
        let system_entries: Vec<(u8, ShipSystem)> = self
            .systems
            .iter()
            .map(|entry| (*entry.key(), entry.value().clone()))
            .collect();
        writer.write_packed_u32(system_entries.len() as u32);

        // Write each system: type byte + system-specific data
        for (sys_type, system) in &system_entries {
            writer.write_byte(*sys_type);
            system.serialize_into(writer);
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
        let map_val = reader.read_byte();
        if let Some(map) = match map_val {
            0 => Some(MapType::Skeld),
            1 => Some(MapType::MiraHQ),
            2 => Some(MapType::Polus),
            3 => Some(MapType::Dleks),
            4 => Some(MapType::Airship),
            5 => Some(MapType::Fungle),
            _ => None,
        } {
            *self.map_type.lock() = map;
        }

        let system_count = reader.read_packed_u32() as usize;
        self.systems.clear();

        for _ in 0..system_count {
            let sys_type = reader.read_byte();
            if let Some(system) = ShipSystem::deserialize_from(sys_type, reader) {
                self.systems.insert(sys_type, system);
            }
        }

        // Recompute sabotage_active flag
        let active = self.systems.iter().any(|entry| entry.value().is_sabotage());
        *self.sabotage_active.write() = active;

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
            RpcCalls::UpdateSystem => {
                let system_type = reader.read_byte();
                let amount = reader.read_byte();
                let player_id: PlayerId = sender.client_id as PlayerId;

                debug!(
                    "game {}: UpdateSystem type={} amount={} player={}",
                    self.game.code, system_type, amount, player_id
                );

                self.update_system(system_type, amount, player_id)?;
                Ok(true)
            }
            RpcCalls::CloseDoorsOfType => {
                let system_type = reader.read_byte();

                debug!(
                    "game {}: CloseDoorsOfType type={} by player {}",
                    self.game.code, system_type, sender.client_id
                );

                self.close_doors_of_type(system_type)?;
                Ok(true)
            }
            _ => {
                // RPC not handled by ship status
                Ok(false)
            }
        }
    }
}
