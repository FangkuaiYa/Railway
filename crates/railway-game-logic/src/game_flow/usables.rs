//! Usable items — interactive objects that players can use on the map.
//!
//! Usables include:
//! - Admin Table (view all player locations)
//! - Vitals Monitor (view alive/dead status)
//! - Door Log (view door open/close history)
//! - Security Cameras (watch camera feeds)
//! - Vending Machines (buy snacks, Airship)
//! - Custom Usables (map-specific interactables)
//!
//! Each usable tracks which player is currently using it and provides
//! methods to activate, deactivate, and check usage status.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use railway_protocol::{ClientId, PlayerId};
use parking_lot::Mutex;

/// Trait for objects that can be activated/deactivated by players.
pub trait Usable: Send + Sync {
    /// The percentage value (0.0 - 1.0) representing how "used" this item is.
    /// 0 = idle, 1 = fully used/active.
    fn percent(&self) -> f32;

    /// The distance at which a player can interact with this usable.
    fn usable_distance(&self) -> f32;

    /// Called when a player begins using this item.
    fn set_use(&mut self, is_using: bool, player_id: Option<PlayerId>);

    /// Called every frame/tick to update the usable's state.
    fn update(&mut self, delta_time: f32);

    /// Returns true if the usable can be used right now.
    fn can_use(&self, player_id: PlayerId) -> bool;

    /// Returns the player currently using this, if any.
    fn current_user(&self) -> Option<PlayerId>;
}

// ── Admin Table ──────────────────────────────────────────────────

/// Admin Table — shows all player locations on the map.
/// Available on Skeld, Mira HQ, Polus.
pub struct AdminTable {
    /// The percentage of the scan (0.0 idle, 1.0 fully revealed).
    percent: Mutex<f32>,
    /// The player currently using the admin table.
    current_user: Mutex<Option<PlayerId>>,
    /// Cooldown between uses.
    cooldown: Mutex<f32>,
    /// Time since last use.
    time_since_last_use: Mutex<f32>,
    /// Usable distance (how close a player must be).
    usable_distance: f32,
}

impl AdminTable {
    pub fn new() -> Self {
        Self {
            percent: Mutex::new(0.0),
            current_user: Mutex::new(None),
            cooldown: Mutex::new(5.0),
            time_since_last_use: Mutex::new(0.0),
            usable_distance: 1.0,
        }
    }
}

impl Usable for AdminTable {
    fn percent(&self) -> f32 {
        *self.percent.lock()
    }

    fn usable_distance(&self) -> f32 {
        self.usable_distance
    }

    fn set_use(&mut self, is_using: bool, player_id: Option<PlayerId>) {
        if is_using {
            *self.current_user.lock() = player_id;
            *self.percent.lock() = 1.0;
        } else {
            *self.current_user.lock() = None;
            *self.percent.lock() = 0.0;
        }
    }

    fn update(&mut self, delta_time: f32) {
        let mut time = self.time_since_last_use.lock();
        *time += delta_time;

        // If not being used, decay the percent
        if self.current_user().is_none() {
            let mut pct = self.percent.lock();
            *pct = (*pct - delta_time * 2.0).max(0.0);
        }
    }

    fn can_use(&self, _player_id: PlayerId) -> bool {
        let time = *self.time_since_last_use.lock();
        let cooldown = *self.cooldown.lock();
        time >= cooldown
    }

    fn current_user(&self) -> Option<PlayerId> {
        *self.current_user.lock()
    }
}

// ── Vitals Monitor ───────────────────────────────────────────────

/// Vitals Monitor — shows which players are alive or dead.
/// Available on Polus and Airship.
pub struct VitalsMonitor {
    percent: Mutex<f32>,
    current_user: Mutex<Option<PlayerId>>,
    usable_distance: f32,
}

impl VitalsMonitor {
    pub fn new() -> Self {
        Self {
            percent: Mutex::new(0.0),
            current_user: Mutex::new(None),
            usable_distance: 1.0,
        }
    }
}

impl Usable for VitalsMonitor {
    fn percent(&self) -> f32 {
        *self.percent.lock()
    }

    fn usable_distance(&self) -> f32 {
        self.usable_distance
    }

    fn set_use(&mut self, is_using: bool, player_id: Option<PlayerId>) {
        if is_using {
            *self.current_user.lock() = player_id;
            *self.percent.lock() = 1.0;
        } else {
            *self.current_user.lock() = None;
            *self.percent.lock() = 0.0;
        }
    }

    fn update(&mut self, _delta_time: f32) {
        // Vitals monitor has no time decay — it shows real-time data
    }

    fn can_use(&self, _player_id: PlayerId) -> bool {
        // Can always be used
        true
    }

    fn current_user(&self) -> Option<PlayerId> {
        *self.current_user.lock()
    }
}

// ── Door Log ─────────────────────────────────────────────────────

/// Door Log — shows a history of door open/close events.
/// Available on Mira HQ.
pub struct DoorLog {
    percent: Mutex<f32>,
    current_user: Mutex<Option<PlayerId>>,
    /// Log entries: (timestamp, door_id, was_opened)
    entries: Mutex<Vec<(Instant, u8, bool)>>,
    usable_distance: f32,
    max_entries: usize,
}

impl DoorLog {
    pub fn new() -> Self {
        Self {
            percent: Mutex::new(0.0),
            current_user: Mutex::new(None),
            entries: Mutex::new(Vec::new()),
            usable_distance: 1.0,
            max_entries: 50,
        }
    }

    /// Add a door event to the log.
    pub fn log_door_event(&self, door_id: u8, was_opened: bool) {
        let mut entries = self.entries.lock();
        entries.push((Instant::now(), door_id, was_opened));

        if entries.len() > self.max_entries {
            entries.remove(0);
        }
    }

    /// Get all log entries.
    pub fn get_entries(&self) -> Vec<(Instant, u8, bool)> {
        self.entries.lock().clone()
    }

    /// Clear the door log.
    pub fn clear(&self) {
        self.entries.lock().clear();
    }
}

impl Usable for DoorLog {
    fn percent(&self) -> f32 {
        *self.percent.lock()
    }

    fn usable_distance(&self) -> f32 {
        self.usable_distance
    }

    fn set_use(&mut self, is_using: bool, player_id: Option<PlayerId>) {
        if is_using {
            *self.current_user.lock() = player_id;
            *self.percent.lock() = 1.0;
        } else {
            *self.current_user.lock() = None;
            *self.percent.lock() = 0.0;
        }
    }

    fn update(&mut self, _delta_time: f32) {}

    fn can_use(&self, _player_id: PlayerId) -> bool {
        true
    }

    fn current_user(&self) -> Option<PlayerId> {
        *self.current_user.lock()
    }
}

// ── Security Camera ──────────────────────────────────────────────

/// Security Camera — view camera feeds on the map.
/// Available on Skeld, Polus, Airship.
pub struct SecurityCamera {
    percent: Mutex<f32>,
    current_user: Mutex<Option<PlayerId>>,
    /// Which camera is currently being viewed (camera index).
    current_camera: Mutex<u8>,
    /// Total number of cameras available.
    camera_count: u8,
    usable_distance: f32,
    is_active: Mutex<bool>,
}

impl SecurityCamera {
    pub fn new(camera_count: u8) -> Self {
        Self {
            percent: Mutex::new(0.0),
            current_user: Mutex::new(None),
            current_camera: Mutex::new(0),
            camera_count,
            usable_distance: 1.0,
            is_active: Mutex::new(false),
        }
    }

    /// Switch to a different camera.
    pub fn set_camera(&self, camera_index: u8) {
        if camera_index < self.camera_count {
            *self.current_camera.lock() = camera_index;
        }
    }

    /// Get the currently active camera index.
    pub fn current_camera(&self) -> u8 {
        *self.current_camera.lock()
    }

    /// Returns the number of cameras.
    pub fn camera_count(&self) -> u8 {
        self.camera_count
    }

    /// Returns whether the security camera console is active (red light on).
    pub fn is_active(&self) -> bool {
        *self.is_active.lock()
    }
}

impl Usable for SecurityCamera {
    fn percent(&self) -> f32 {
        *self.percent.lock()
    }

    fn usable_distance(&self) -> f32 {
        self.usable_distance
    }

    fn set_use(&mut self, is_using: bool, player_id: Option<PlayerId>) {
        *self.is_active.lock() = is_using;
        if is_using {
            *self.current_user.lock() = player_id;
            *self.percent.lock() = 1.0;
        } else {
            *self.current_user.lock() = None;
            *self.percent.lock() = 0.0;
        }
    }

    fn update(&mut self, _delta_time: f32) {}

    fn can_use(&self, _player_id: PlayerId) -> bool {
        self.current_user().is_none()
    }

    fn current_user(&self) -> Option<PlayerId> {
        *self.current_user.lock()
    }
}

// ── Vending Machine ──────────────────────────────────────────────

/// Vending Machine — on the Airship, players can buy snacks.
pub struct VendingMachine {
    percent: Mutex<f32>,
    current_user: Mutex<Option<PlayerId>>,
    usable_distance: f32,
    /// Number of items remaining.
    items_remaining: Mutex<u32>,
    total_items: u32,
}

impl VendingMachine {
    pub fn new(total_items: u32) -> Self {
        Self {
            percent: Mutex::new(0.0),
            current_user: Mutex::new(None),
            usable_distance: 1.0,
            items_remaining: Mutex::new(total_items),
            total_items,
        }
    }

    /// Purchase an item from the vending machine.
    /// Returns true if successful, false if out of stock.
    pub fn purchase(&self) -> bool {
        let mut remaining = self.items_remaining.lock();
        if *remaining > 0 {
            *remaining -= 1;
            true
        } else {
            false
        }
    }

    /// Restock the vending machine.
    pub fn restock(&self) {
        *self.items_remaining.lock() = self.total_items;
    }

    /// Get remaining items.
    pub fn items_remaining(&self) -> u32 {
        *self.items_remaining.lock()
    }
}

impl Usable for VendingMachine {
    fn percent(&self) -> f32 {
        *self.percent.lock()
    }

    fn usable_distance(&self) -> f32 {
        self.usable_distance
    }

    fn set_use(&mut self, is_using: bool, player_id: Option<PlayerId>) {
        if is_using {
            *self.current_user.lock() = player_id;
            *self.percent.lock() = 1.0;
        } else {
            *self.current_user.lock() = None;
            *self.percent.lock() = 0.0;
        }
    }

    fn update(&mut self, _delta_time: f32) {}

    fn can_use(&self, _player_id: PlayerId) -> bool {
        *self.items_remaining.lock() > 0
    }

    fn current_user(&self) -> Option<PlayerId> {
        *self.current_user.lock()
    }
}

// ── Custom Usable ────────────────────────────────────────────────

/// A custom/generic usable for map-specific interactables.
pub struct CustomUsable {
    percent: Mutex<f32>,
    current_user: Mutex<Option<PlayerId>>,
    usable_distance: f32,
}

impl CustomUsable {
    pub fn new(usable_distance: f32) -> Self {
        Self {
            percent: Mutex::new(0.0),
            current_user: Mutex::new(None),
            usable_distance,
        }
    }
}

impl Usable for CustomUsable {
    fn percent(&self) -> f32 {
        *self.percent.lock()
    }

    fn usable_distance(&self) -> f32 {
        self.usable_distance
    }

    fn set_use(&mut self, is_using: bool, player_id: Option<PlayerId>) {
        if is_using {
            *self.current_user.lock() = player_id;
            *self.percent.lock() = 1.0;
        } else {
            *self.current_user.lock() = None;
            *self.percent.lock() = 0.0;
        }
    }

    fn update(&mut self, _delta_time: f32) {}

    fn can_use(&self, _player_id: PlayerId) -> bool {
        true
    }

    fn current_user(&self) -> Option<PlayerId> {
        *self.current_user.lock()
    }
}

// ── Usable Manager ───────────────────────────────────────────────

/// Tracks which player is using which usable object at any given time.
pub struct UsableManager {
    /// Map of usable object ID -> usable.
    pub usables: Mutex<HashMap<u32, Arc<dyn Usable>>>,
    /// Tracks which client is using which usable.
    pub player_using: Mutex<HashMap<ClientId, u32>>,
}

impl UsableManager {
    pub fn new() -> Self {
        Self {
            usables: Mutex::new(HashMap::new()),
            player_using: Mutex::new(HashMap::new()),
        }
    }

    /// Register a usable object.
    pub fn register(&self, id: u32, usable: Arc<dyn Usable>) {
        self.usables.lock().insert(id, usable);
    }

    /// Unregister a usable object.
    pub fn unregister(&self, id: u32) {
        self.usables.lock().remove(&id);
    }

    /// Check if a player can use a specific usable.
    pub fn can_use(&self, player_id: PlayerId, usable_id: u32) -> bool {
        let usables = self.usables.lock();
        if let Some(usable) = usables.get(&usable_id) {
            usable.can_use(player_id)
        } else {
            false
        }
    }

    /// Mark a player as using a specific usable.
    pub fn set_player_using(&self, client_id: ClientId, usable_id: u32) {
        let mut using = self.player_using.lock();

        // If player was using another usable, deactivate it first
        let prev = using.get(&client_id).copied();
        if let Some(prev_id) = prev {
            if prev_id != usable_id {
                // Release any previous usable usage
                drop(using);
                // Re-acquire after dropping
                let mut using = self.player_using.lock();
                using.insert(client_id, usable_id);
                return;
            }
        }

        using.insert(client_id, usable_id);
    }

    /// Clear a player's usage state.
    pub fn clear_player_using(&self, client_id: ClientId) {
        self.player_using.lock().remove(&client_id);
    }

    /// Get the usable ID that a player is currently using.
    pub fn get_player_usable(&self, client_id: ClientId) -> Option<u32> {
        self.player_using.lock().get(&client_id).copied()
    }

    /// Update all usables (called each tick).
    pub fn update_all(&self, _delta_time: f32) {
        let _usables = self.usables.lock();
        // Each usable implements interior mutability for state updates.
        // In a full implementation, each Usable would be called with:
        //   _usable.update(_delta_time);
    }
}

impl Default for UsableManager {
    fn default() -> Self {
        Self::new()
    }
}
