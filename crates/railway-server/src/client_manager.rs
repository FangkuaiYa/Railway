//! Client manager: tracks connected clients, assigns IDs, handles version checking.

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use tracing::{info, warn};

use railway_protocol::messages::c2s::handshake::HandshakeData;

use crate::client::Client;
use crate::config::{AntiCheatConfig, CompatibilityConfig};

/// Manages all connected clients.
pub struct ClientManager {
    /// All connected clients, keyed by client ID.
    clients: DashMap<i32, Arc<Client>>,
    /// Next client ID counter.
    next_id: AtomicI32,
    /// Compatibility settings.
    compatibility: CompatibilityConfig,
    /// Anti-cheat settings. Previously only read once at startup for a log
    /// line — the actual check functions never consulted these flags/
    /// thresholds at all (hardcoded values were used instead).
    pub anticheat: AntiCheatConfig,
    /// Total connections accepted (monotonic counter).
    total_connections: AtomicI32,
    /// Total disconnections.
    total_disconnections: AtomicI32,
}

impl ClientManager {
    pub fn new(compatibility: CompatibilityConfig, anticheat: AntiCheatConfig) -> Self {
        Self {
            clients: DashMap::new(),
            next_id: AtomicI32::new(1),
            compatibility,
            anticheat,
            total_connections: AtomicI32::new(0),
            total_disconnections: AtomicI32::new(0),
        }
    }

    /// Generate the next client ID.
    pub fn next_id(&self) -> i32 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        if id < 1 {
            // Overflow safety: reset
            self.next_id.store(1, Ordering::SeqCst);
            return self.next_id();
        }
        id
    }

    /// Register a new client. Returns true if a new connection was registered.
    pub fn register(&self, client: Arc<Client>) -> bool {
        let id = client.id;
        if self.clients.contains_key(&id) {
            warn!("client {} already registered", id);
            return false;
        }
        self.clients.insert(id, client);
        self.total_connections.fetch_add(1, Ordering::SeqCst);
        info!(
            "client {} registered (total: {}, active: {})",
            id,
            self.total_connections.load(Ordering::SeqCst),
            self.clients.len()
        );
        true
    }

    /// Remove a client by ID. Returns the removed client if it existed.
    pub fn remove(&self, client_id: i32) -> Option<Arc<Client>> {
        let removed = self.clients.remove(&client_id).map(|(_, c)| c);
        if removed.is_some() {
            self.total_disconnections.fetch_add(1, Ordering::SeqCst);
            info!(
                "client {} removed (total disconnects: {}, active: {})",
                client_id,
                self.total_disconnections.load(Ordering::SeqCst),
                self.clients.len()
            );
        }
        removed
    }

    /// Get a client by ID.
    pub fn get(&self, client_id: i32) -> Option<Arc<Client>> {
        self.clients.get(&client_id).map(|r| Arc::clone(r.value()))
    }

    /// Validate that a client is properly registered.
    pub fn validate(&self, client_id: i32) -> bool {
        self.clients.contains_key(&client_id)
    }

    /// Returns the total number of currently connected clients.
    pub fn active_count(&self) -> usize {
        self.clients.len()
    }

    /// Returns a reference to the compatibility configuration.
    pub fn compatibility(&self) -> &CompatibilityConfig {
        &self.compatibility
    }

    /// Returns total connections ever accepted.
    pub fn total_connections(&self) -> i32 {
        self.total_connections.load(Ordering::SeqCst)
    }

    /// Returns total disconnections.
    pub fn total_disconnections(&self) -> i32 {
        self.total_disconnections.load(Ordering::SeqCst)
    }

    /// Check if a client's version is compatible with the server.
    pub fn check_version(&self, handshake: &HandshakeData) -> VersionCheck {
        let version = handshake.client_version;

        // Basic version check — allow any recent version
        if version < railway_protocol::GameVersion::V1 {
            return VersionCheck::Reject("client version too old");
        }

        // In production, we'd check against a known compatible version list
        VersionCheck::Accept
    }

    /// Check if a name is valid for the anticheat.
    pub fn check_name(&self, name: &str) -> bool {
        !name.is_empty() && name.len() <= 10 && !name.trim().is_empty()
    }

    /// Iterate over all connected clients (snapshot).
    pub fn all_clients(&self) -> Vec<Arc<Client>> {
        self.clients.iter().map(|r| Arc::clone(r.value())).collect()
    }
}

/// Result of a version compatibility check.
#[derive(Debug)]
pub enum VersionCheck {
    Accept,
    Reject(&'static str),
}
