//! # railway-game-logic
//!
//! Pure Among Us game logic with **zero networking code**.
//!
//! This crate manages:
//! - Game state machine (NotStarted → Starting → Started → Ended → Destroyed)
//! - Player management (join, leave, limbo states)
//! - InnerNetObject registry and lifecycle (spawn, despawn, serialize)
//! - RPC handling on game objects
//! - Game flow logic for Normal and Hide & Seek modes
//! - Anticheat validation
//! - Event bus for plugins
//!
//! All state changes are expressed as pure data transformations.
//! The `railway-server` crate handles all I/O and message dispatch.

pub mod anticheat;
pub mod error;
pub mod events;
pub mod game;
pub mod game_flow;
pub mod limbo_state;
pub mod objects;
pub mod player;
pub mod state;

pub use error::GameError;
pub use game::Game;
pub use limbo_state::LimboStates;
pub use player::ClientPlayer;
pub use state::GameState;

pub use railway_protocol::{ClientId, GameCode, NetId, PlayerId};
use railway_protocol::MIN_SERVER_NET_ID;

/// Result type alias for game logic operations.
pub type GameResult<T> = Result<T, GameError>;

/// Special owner IDs for InnerNetObject ownership.
pub const HOST_INHERIT_ID: ClientId = -2;
pub const CURRENT_CLIENT_ID: ClientId = -3;
pub const SERVER_OWNED_ID: ClientId = -4;
const MIN_SERVER_NET_ID_CONST: NetId = MIN_SERVER_NET_ID as NetId;

// ── Convenience re-exports ──────────────────────────────────────────

/// Re-export of the anti-cheat check functions for ergonomic use.
///
/// Allows callers to write `use railway_game_logic::anticheat_checks::*;`
/// instead of drilling into the module hierarchy.
pub use anticheat::checks as anticheat_checks;

/// Re-export of [`CheatResult`](anticheat::CheatResult) so callers can use
/// it without importing the `anticheat` module directly.
pub use anticheat::CheatResult;
