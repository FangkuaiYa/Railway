//! Game manager: creates, finds, and destroys games.

use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::RwLock;
use rand::Rng;
use rand::SeedableRng;
use tracing::{debug, info, warn};

use railway_game_logic::Game;
use railway_protocol::{
    ClientId, GameCode,
    game_options::GameOptionsData,
    messages::c2s::host_game::GameFilterOptions,
};

/// Manages all active games.
pub struct GameManager {
    /// Active games, keyed by game code.
    games: DashMap<GameCode, Arc<Game>>,
    /// Maps game code -> host client ID (who created it).
    game_hosts: DashMap<GameCode, ClientId>,
    /// Random generator for game codes.
    rng: RwLock<rand::rngs::StdRng>,
}

impl GameManager {
    pub fn new() -> Self {
        Self {
            games: DashMap::new(),
            game_hosts: DashMap::new(),
            rng: RwLock::new(rand::rngs::StdRng::from_entropy()),
        }
    }

    /// Generate a unique V2 6-letter game code.
    ///
    /// Among Us V2 encodes a 6-letter room code as a NEGATIVE i32 with the
    /// highest bit set (`V2Flag = int.MinValue`).  Positive codes are treated
    /// as legacy V1 4-byte-char codes and the client will show "local" for
    /// any code whose bytes aren't all in 'A'..'z'.
    ///
    /// V2 layout (C# `CreateGameId`):
    /// ```csharp
    /// int.MinValue | (gn & 1023) | ((sn & 1048575) << 10)
    /// ```
    /// where `gn` encodes 2 letters and `sn` encodes 4 letters, both in a
    /// custom 26-char alphabet `"QWXRTYLPESDFGHUJKZOCVBINMA"`.
    pub fn generate_code(&self) -> GameCode {
        const V2_FLAG: i32 = i32::MIN; // 0x80000000
        let mut rng = self.rng.write();
        for _ in 0..100 {
            // gn: 2 letters in custom base-26 → 0..675
            let gn: i32 = rng.gen_range(0..676);
            // sn: 4 letters in custom base-26 → 0..456976
            let sn: i32 = rng.gen_range(0..456976);
            let code: GameCode = V2_FLAG | (gn & 1023) | ((sn & 1048575) << 10);
            if !self.games.contains_key(&code) {
                return code;
            }
        }
        // Fallback: brute-force (extremely unlikely to be reached)
        for sn in 0..456976i32 {
            for gn in 0..676i32 {
                let code = V2_FLAG | (gn & 1023) | ((sn & 1048575) << 10);
                if !self.games.contains_key(&code) {
                    return code;
                }
            }
        }
        // Should never happen
        warn!("all V2 game codes exhausted");
        V2_FLAG
    }

    /// Create a new game with the given options and host.
    ///
    /// Matches C# Impostor: the game is created with `HostId = -1` and
    /// the first player to join (via JoinGame) becomes the host.  We do
    /// NOT call `set_host_id` here.
    pub fn create(
        &self,
        options: Arc<dyn GameOptionsData>,
        filter_options: GameFilterOptions,
        host_client_id: ClientId,
    ) -> Option<Arc<Game>> {
        for _ in 0..10 {
            let code = self.generate_code();

            // IMPORTANT: must be an atomic "insert only if absent", matching
            // C#'s `_games.TryAdd(gameCode, game)`. A previous version used
            // `DashMap::insert`, which unconditionally OVERWRITES whatever
            // was already at that key and only afterwards reports (via the
            // returned Option) whether there had been a collision — by
            // which point a real, possibly-occupied game already got
            // clobbered. `entry()` lets us check-and-insert atomically
            // without ever touching an existing entry.
            let inserted = match self.games.entry(code) {
                dashmap::mapref::entry::Entry::Occupied(_) => false,
                dashmap::mapref::entry::Entry::Vacant(slot) => {
                    let game = Game::new(code, Arc::clone(&options), filter_options.clone());
                    slot.insert(game);
                    true
                }
            };

            if inserted {
                let game = self.games.get(&code).map(|r| Arc::clone(r.value()))?;
                // Track which client created this game (for cleanup)
                self.game_hosts.insert(code, host_client_id);
                // HostId stays -1 until PlayerAdd in handle_join_game
                debug!(
                    "created game {} (creator: {}, total games: {})",
                    code,
                    host_client_id,
                    self.games.len()
                );
                info!(
                    "game {} created: filters={:?} mode={:?}",
                    code,
                    game.filter_options.read().filter_tags,
                    game.options.game_mode(),
                );
                return Some(game);
            }
        }

        info!("failed to create game (all codes taken)");
        None
    }

    /// Find a game by its code.
    pub fn find(&self, code: GameCode) -> Option<Arc<Game>> {
        self.games.get(&code).map(|r| Arc::clone(r.value()))
    }

    /// Remove a game (destroy it).
    pub fn remove(&self, code: GameCode) -> Option<Arc<Game>> {
        if let Some((_, game)) = self.games.remove(&code) {
            self.game_hosts.remove(&code);
            game.set_state(railway_game_logic::GameState::Destroyed);
            game.emit_event(railway_game_logic::events::GameEvent::GameDestroyed {
                game_code: code,
            });
            debug!(
                "removed game {} (remaining: {})",
                code,
                self.games.len()
            );
            Some(game)
        } else {
            None
        }
    }

    /// Check if a client is the host of a given game.
    pub fn is_host(&self, code: GameCode, client_id: ClientId) -> bool {
        self.game_hosts
            .get(&code)
            .map(|r| *r.value() == client_id)
            .unwrap_or(false)
    }

    /// Get the host client ID for a game.
    pub fn host_id(&self, code: GameCode) -> Option<ClientId> {
        self.game_hosts.get(&code).map(|r| *r.value())
    }

    /// Auto-destroy empty games (call periodically or after player leaves).
    pub fn cleanup_empty(&self) -> usize {
        let mut removed = 0;
        self.games.retain(|code, game| {
            if game.player_count() == 0 {
                game.set_state(railway_game_logic::GameState::Destroyed);
                game.emit_event(railway_game_logic::events::GameEvent::GameDestroyed {
                    game_code: *code,
                });
                self.game_hosts.remove(code);
                removed += 1;
                debug!("auto-destroyed empty game {}", code);
                false
            } else {
                true
            }
        });
        removed
    }

    /// Returns the number of active games.
    pub fn game_count(&self) -> usize {
        self.games.len()
    }

    /// Returns an iterator over all active games.
    pub fn all_games(&self) -> Vec<Arc<Game>> {
        self.games.iter().map(|r| Arc::clone(r.value())).collect()
    }

    /// Returns a list of public games suitable for the HTTP API.
    pub fn public_games(&self, client_manager: &crate::client_manager::ClientManager) -> Vec<PublicGameInfo> {
        self.games
            .iter()
            .filter(|r| r.value().is_public.load(std::sync::atomic::Ordering::Relaxed))
            .map(|r| Self::game_info_from(r.value(), client_manager))
            .collect()
    }

    /// Build a `PublicGameInfo` snapshot from a live `Game`, pulling real
    /// settings (host name, max players, map, impostor count) instead of
    /// placeholder values. Used both by `public_games()` (lobby search)
    /// and by direct join-by-code lookups (`show_game`), since joining a
    /// specific game by its code should show real info even if the game
    /// isn't public.
    pub fn game_info_from(game: &Arc<Game>, client_manager: &crate::client_manager::ClientManager) -> PublicGameInfo {
        // Extract game settings from the game options (not filter options).
        let (max_players, map, num_impostors, keywords) =
            if let Some(normal) = game.options.as_any().downcast_ref::<railway_protocol::game_options::NormalGameOptions>() {
                (normal.max_players, normal.map, normal.num_impostors, normal.keywords)
            } else if let Some(hns) = game.options.as_any().downcast_ref::<railway_protocol::game_options::HideNSeekGameOptions>() {
                (hns.max_players, hns.map, 1u8, hns.keywords) // HnS always has 1 seeker
            } else {
                (10u8, 0u8, 2u8, 0u32) // defaults
            };
        let host_client_id = game.players.iter().find(|p| p.is_host).map(|p| p.client_id);
        let host_name = host_client_id
            .and_then(|id| game.players.get(&id).map(|p| p.name.clone()))
            .unwrap_or_else(|| "Unknown".to_string());

        // Matches C#'s `GameListing.From`: `QuickChat`/platform info come
        // from the HOST's own `Client` (chat_mode from their handshake,
        // platform data likewise) — not from the game itself.
        let (chat_mode, host_platform_name, host_platform) = host_client_id
            .and_then(|id| client_manager.get(id))
            .map(|c| {
                let (pname, ptype) = c
                    .platform_data
                    .as_ref()
                    .map(|p| (p.platform_name.clone(), p.platform.to_u32()))
                    .unwrap_or_default();
                (c.chat_mode as u8, pname, ptype)
            })
            .unwrap_or((1u8, String::new(), 0u32)); // default QuickChatOnly=1

        PublicGameInfo {
            code: game.code,
            player_count: game.player_count() as u8,
            max_players,
            map,
            num_impostors,
            host_name,
            keywords,
            chat_mode,
            host_platform_name,
            host_platform,
        }
    }
}

/// Public game information for listing APIs.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PublicGameInfo {
    pub code: GameCode,
    pub player_count: u8,
    pub max_players: u8,
    pub map: u8,
    pub num_impostors: u8,
    pub host_name: String,
    /// `GameOptions.Keywords` bitmask (language/chat-language selection).
    pub keywords: u32,
    /// Host's chat mode (QuickChatModes), from their handshake.
    pub chat_mode: u8,
    pub host_platform_name: String,
    pub host_platform: u32,
}
