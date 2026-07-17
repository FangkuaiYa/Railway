//! Game state container.
//!
//! The `Game` struct holds all state for a single game session:
//! players, game objects, state machine, event bus, and anti-cheat state.
//!
//! All mutable state uses `parking_lot` locks or `DashMap` for concurrent access.
//! The game has **zero knowledge of networking** — it only manipulates its own state
//! and emits events.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{debug, info};

use railway_protocol::{
    ClientId, DisconnectReason, GameCode, NetId,
    game_options::GameOptionsData,
    messages::c2s::host_game::GameFilterOptions,
};

use crate::error::GameError;
use crate::events::GameEvent;
use crate::objects::InnerNetObject;
use crate::player::ClientPlayer;
use crate::state::GameState;
use crate::GameResult;

/// The main game container. Always held behind `Arc<Game>`.
pub struct Game {
    /// The game code (lobby code).
    pub code: GameCode,

    /// Current game state.
    pub state: RwLock<GameState>,

    /// Whether the game is public (visible in listings).
    pub is_public: AtomicBool,

    /// The host player's client ID. -1 means no host.
    pub host_id: AtomicI32,

    /// Display name for the game.
    pub display_name: RwLock<Option<String>>,

    /// Game options (Normal or HideAndSeek), shared via Arc.
    pub options: Arc<dyn GameOptionsData>,

    /// Filter options for public listings.
    pub filter_options: RwLock<GameFilterOptions>,

    /// Players in the game, keyed by client ID.
    pub players: DashMap<ClientId, ClientPlayer>,

    /// All InnerNetObjects in the game, keyed by NetId.
    pub objects: DashMap<NetId, Arc<dyn InnerNetObject>>,

    /// Owner ClientId for each NetId, as reported by the SpawnFlag that
    /// created it. Used for RPC ownership validation (see
    /// `anticheat_checks::check_ownership`) — this was tracked nowhere
    /// before, so no RPC ever verified the sender actually owned the
    /// object it was acting on.
    pub net_id_owners: DashMap<NetId, ClientId>,

    /// Raw bytes of the SpawnFlag payload (objectId..last component) for
    /// every currently-live SERVER-OWNED object (VoteBanSystem,
    /// NormalGameManager, LobbyBehaviour, GameData, etc — anything with
    /// owner == SERVER_OWNED_ID), keyed by that object's first NetId.
    /// Replayed verbatim to late-joining clients — matches C#'s
    /// `Game.Data.cs: SyncServerObjectsAsync`, which resends a spawn
    /// message for every server-owned object whenever someone new sends
    /// SceneChangeFlag. Without this, a second player joining never
    /// learns these core objects exist at all.
    pub server_owned_spawn_cache: DashMap<NetId, bytes::Bytes>,

    /// Banned IP addresses.
    pub banned_ips: RwLock<HashSet<IpAddr>>,

    /// Next NetId to assign for server-owned objects.
    next_net_id: AtomicU32,

    /// Event bus for game lifecycle events.
    event_tx: broadcast::Sender<GameEvent>,
}

impl Game {
    /// Create a new game.
    pub fn new(
        code: GameCode,
        options: Arc<dyn GameOptionsData>,
        filter_options: GameFilterOptions,
    ) -> Arc<Self> {
        let (event_tx, _) = broadcast::channel(256);

        Arc::new(Self {
            code,
            state: RwLock::new(GameState::NotStarted),
            is_public: AtomicBool::new(false),
            host_id: AtomicI32::new(-1),
            display_name: RwLock::new(None),
            options,
            filter_options: RwLock::new(filter_options),
            players: DashMap::new(),
            objects: DashMap::new(),
            net_id_owners: DashMap::new(),
            server_owned_spawn_cache: DashMap::new(),
            banned_ips: RwLock::new(HashSet::new()),
            next_net_id: AtomicU32::new(crate::MIN_SERVER_NET_ID_CONST),
            event_tx,
        })
    }

    /// Subscribe to game events.
    pub fn subscribe_events(&self) -> broadcast::Receiver<GameEvent> {
        self.event_tx.subscribe()
    }

    /// Publish a game event.
    pub fn emit_event(&self, event: GameEvent) {
        let _ = self.event_tx.send(event);
    }

    /// Get the current host ID.
    pub fn host_id(&self) -> ClientId {
        self.host_id.load(Ordering::SeqCst)
    }

    /// Set the host ID.
    pub fn set_host_id(&self, id: ClientId) {
        self.host_id.store(id, Ordering::SeqCst);
    }

    /// Get the current game state.
    pub fn state(&self) -> GameState {
        *self.state.read()
    }

    /// Set the game state.
    pub fn set_state(&self, new_state: GameState) {
        let mut state = self.state.write();
        debug!("game {}: {:?} -> {:?}", self.code, *state, new_state);
        *state = new_state;
    }

    /// Get the player count.
    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    /// Allocate the next server-owned NetId.
    pub fn next_net_id(&self) -> NetId {
        self.next_net_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Find the first free PlayerId slot (0-255), matching real C#'s
    /// `InnerGameData.GetNextAvailablePlayerId` exactly: scan from 0
    /// upward and return the first value not currently in use. This is
    /// NOT the same as "how many players currently have a slot" — that
    /// approach breaks the moment anyone leaves and someone else joins,
    /// since the freed slot must be reused rather than skipped past.
    pub fn next_available_player_id(&self) -> u8 {
        let mut used: Vec<u8> = self
            .players
            .iter()
            .filter_map(|p| p.player_id_slot)
            .collect();
        used.sort_unstable();
        for i in 0..u8::MAX {
            if used.binary_search(&i).is_err() {
                return i;
            }
        }
        u8::MAX
    }

    /// Add a player to the game.
    pub fn add_player(self: &Arc<Self>, client_id: ClientId, name: String) -> GameResult<()> {
        let state = self.state();

        if !state.can_join() {
            return Err(GameError::GameAlreadyStarted(self.code));
        }

        // Use the REAL configured max_players from game options, not a
        // hardcoded placeholder. A previous version of this check ignored
        // `self.options` entirely and always allowed up to 15 players
        // regardless of what the host actually configured.
        let max = self
            .options
            .as_any()
            .downcast_ref::<railway_protocol::game_options::NormalGameOptions>()
            .map(|o| o.max_players)
            .or_else(|| {
                self.options
                    .as_any()
                    .downcast_ref::<railway_protocol::game_options::HideNSeekGameOptions>()
                    .map(|o| o.max_players)
            })
            .unwrap_or(15) as usize;

        if self.players.len() >= max {
            return Err(GameError::GameFull {
                code: self.code,
                count: self.players.len(),
                max,
            });
        }

        let player_name = name.clone();
        // Pass the REAL live game (same Arc, same players/objects maps) —
        // a previous version constructed a disconnected "shallow clone"
        // of the game (empty players/objects, NetId counter reset to 0)
        // just to satisfy `ClientPlayer::new`'s signature. That clone
        // was never kept in sync with the actual game, so any future code
        // reading `player.game.*` would see stale/empty state instead of
        // the real thing.
        let player = ClientPlayer::new(client_id, name, Arc::clone(self));
        self.players.insert(client_id, player);

        // If this player is the host (either newly becoming host or already
        // designated as host), mark them as such.
        if self.host_id() == -1 {
            // First player to join — make them the host
            self.set_host_id(client_id);
        }
        if self.host_id() == client_id {
            if let Some(mut p) = self.players.get_mut(&client_id) {
                p.is_host = true;
                info!("game {}: player {} IS_HOST=true", self.code, client_id);
            }
        }

        info!("game {}: player {} ({}) joined (player_count={}, host_id={})", self.code, player_name, client_id, self.players.len(), self.host_id());
        self.emit_event(GameEvent::PlayerJoined { game_code: self.code, client_id });

        Ok(())
    }

    /// Remove a player from the game.
    pub fn remove_player(&self, client_id: ClientId, reason: DisconnectReason) -> GameResult<()> {
        let removed = self.players.remove(&client_id);

        // Matches the intent of C#'s `DespawnPlayerInfoAsync`: once a
        // player leaves, their server-owned PlayerInfo should stop being
        // replayed to future joiners via the SyncServerObjectsAsync-style
        // cache. We don't yet broadcast an actual despawn message to
        // currently-connected clients (a further gap), but at minimum we
        // must stop caching stale data for people who already left.
        if let Some((_, player)) = &removed {
            if let Some(net_id) = player.player_info_net_id {
                self.server_owned_spawn_cache.remove(&net_id);
            }
        }

        info!("game {}: player {} left ({:?})", self.code, client_id, reason);

        // If the host left, assign a new host
        if self.host_id() == client_id {
            let new_host = self.players.iter().next().map(|p| p.client_id).unwrap_or(-1);
            self.set_host_id(new_host);
            if new_host != -1 {
                if let Some(mut p) = self.players.get_mut(&new_host) {
                    p.is_host = true;
                }
            }
            self.emit_event(GameEvent::HostChanged {
                game_code: self.code,
                new_host_id: new_host,
            });
        }

        // If no players remain, destroy the game
        if self.players.is_empty() {
            self.set_state(GameState::Destroyed);
            self.emit_event(GameEvent::GameDestroyed { game_code: self.code });
        }

        self.emit_event(GameEvent::PlayerLeft {
            game_code: self.code,
            client_id,
            reason,
        });

        Ok(())
    }

    /// Register a game object.
    pub fn register_object(&self, net_id: NetId, obj: Arc<dyn InnerNetObject>) -> bool {
        self.objects.insert(net_id, obj).is_none()
    }

    /// Unregister a game object.
    pub fn unregister_object(&self, net_id: NetId) -> bool {
        self.objects.remove(&net_id).is_some()
    }

    /// Find a game object by its NetId.
    pub fn find_object(&self, net_id: NetId) -> Option<Arc<dyn InnerNetObject>> {
        self.objects.get(&net_id).map(|r| Arc::clone(r.value()))
    }

    /// Ban an IP address from rejoining.
    pub fn ban_ip(&self, ip: IpAddr) {
        self.banned_ips.write().insert(ip);
    }

    /// Check if an IP is banned.
    pub fn is_ip_banned(&self, ip: IpAddr) -> bool {
        self.banned_ips.read().contains(&ip)
    }
}
