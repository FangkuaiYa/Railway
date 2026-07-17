//! Client connection handler.
//!
//! Each `Client` wraps a Hazel `Connection` and holds player state.
//! Messages are dispatched through a `MessageRouter`.

use std::sync::Arc;

use railway_hazel::connection::Connection;
use railway_protocol::{
    GameVersion, Language, QuickChatModes,
    messages::c2s::handshake::HandshakeData,
    platform_data::PlatformSpecificData,
};
use crate::reactor::ReactorMod;

/// A connected game client.
pub struct Client {
    pub id: i32,
    pub name: String,
    pub game_version: GameVersion,
    pub language: Language,
    pub chat_mode: QuickChatModes,
    pub platform_data: Option<PlatformSpecificData>,
    pub connection: Arc<Connection>,
    pub current_game_code: parking_lot::RwLock<Option<i32>>,
    pub disposed: parking_lot::RwLock<bool>,
    /// Reactor mod list for this client (None = vanilla or unparseable).
    pub reactor_mods: Option<Vec<ReactorMod>>,
}

impl Client {
    /// Create a new client from handshake data.
    pub fn new(
        id: i32,
        connection: Arc<Connection>,
        handshake: HandshakeData,
        reactor_mods: Option<Vec<ReactorMod>>,
    ) -> Self {
        Self {
            id,
            name: handshake.name,
            game_version: handshake.client_version,
            language: handshake.language,
            chat_mode: handshake.chat_mode,
            platform_data: handshake.platform_data,
            connection,
            current_game_code: parking_lot::RwLock::new(None),
            disposed: parking_lot::RwLock::new(false),
            reactor_mods,
        }
    }

    /// Returns the client's display name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the game version.
    pub fn game_version(&self) -> GameVersion {
        self.game_version
    }

    /// Check if this client is the host authority (disable server authority flag).
    pub fn is_host_authority(&self) -> bool {
        self.game_version.has_disable_server_authority()
    }

    /// Set the current game code for this client.
    pub fn set_game_code(&self, code: Option<i32>) {
        *self.current_game_code.write() = code;
    }

    /// Get the current game code.
    pub fn game_code(&self) -> Option<i32> {
        *self.current_game_code.read()
    }

    /// Mark this client as disposed and clean up.
    pub fn dispose(&self) {
        let mut disposed = self.disposed.write();
        if *disposed {
            return;
        }
        *disposed = true;
        drop(disposed);

        // Clean up the underlying Hazel connection
        self.connection.dispose();

        // Clear game code reference
        *self.current_game_code.write() = None;
    }

    /// Returns true if this client has been disposed.
    pub fn is_disposed(&self) -> bool {
        *self.disposed.read()
    }
}
