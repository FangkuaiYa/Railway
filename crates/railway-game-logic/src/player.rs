//! Player within a game — tracks limbo state, character reference, etc.

use std::sync::Arc;
use railway_protocol::ClientId;
use crate::game::Game;
use crate::limbo_state::LimboStates;

/// A player within a game context.
///
/// Each player has a limbo state, an optional character (InnerPlayerControl),
/// and can be the host of the game.
#[derive(Clone)]
pub struct ClientPlayer {
    /// The player's client ID.
    pub client_id: ClientId,

    /// The player's display name.
    pub name: String,

    /// The game this player belongs to.
    pub game: Arc<Game>,

    /// Current limbo state.
    pub limbo: LimboStates,

    /// Whether this player is the game host.
    pub is_host: bool,

    /// Whether the player is an impostor.
    pub is_impostor: bool,

    /// The player's character net ID (InnerPlayerControl).
    pub character_net_id: Option<u32>,

    /// The player's PlayerInfo net ID.
    pub player_info_net_id: Option<u32>,

    /// The player's numeric PlayerId slot (0-255), matching real C#'s
    /// `InnerPlayerInfo.PlayerId`. Must be tracked explicitly (not
    /// recomputed as "how many players currently have one") because slots
    /// get freed when players leave and must be REUSED for the next
    /// joiner — see `Game::next_available_player_id`.
    pub player_id_slot: Option<u8>,

    /// The scene the player is currently in (e.g. "OnlineGame").
    pub scene: Option<String>,
}

impl ClientPlayer {
    /// Create a new player in PreSpawn limbo.
    pub fn new(client_id: ClientId, name: String, game: Arc<Game>) -> Self {
        Self {
            client_id,
            name,
            game,
            limbo: LimboStates::PreSpawn,
            is_host: false,
            is_impostor: false,
            character_net_id: None,
            player_info_net_id: None,
            player_id_slot: None,
            scene: None,
        }
    }
}
