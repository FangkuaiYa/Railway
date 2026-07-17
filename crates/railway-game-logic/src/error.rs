//! Error types for game logic operations.

use thiserror::Error;
use railway_protocol::GameCode;

/// Errors that can occur in game logic.
#[derive(Error, Debug)]
pub enum GameError {
    #[error("game {0} not found")]
    GameNotFound(GameCode),

    #[error("game {code}: {message}")]
    GameLogic { code: GameCode, message: String },

    #[error("player {0} not found in game")]
    PlayerNotFound(i32),

    #[error("object with net_id {0} not found")]
    ObjectNotFound(u32),

    #[error("invalid state transition from {from:?} to {to:?}")]
    InvalidStateTransition { from: String, to: String },

    #[error("game {code} is full ({count}/{max})")]
    GameFull { code: GameCode, count: usize, max: usize },

    #[error("player is banned from game {code}")]
    PlayerBanned { code: GameCode },

    #[error("game {0} has already started")]
    GameAlreadyStarted(GameCode),

    #[error("cheat detected: {message}")]
    CheatDetected { message: String },

    #[error("host-only operation attempted by non-host player {0}")]
    HostOnlyOperation(i32),

    #[error("invalid RPC: {0}")]
    InvalidRpc(String),

    #[error("serialization error: {0}")]
    SerializationError(String),

    #[error("anticheat: {0}")]
    AntiCheatError(String),

    #[error("join error: {0}")]
    JoinError(String),
}

/// Reasons a join can fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameJoinError {
    None,
    InvalidClient,
    Banned,
    GameFull,
    InvalidLimbo,
    GameStarted,
    GameDestroyed,
    ClientOutdated,
    ClientTooNew,
    Custom,
}

impl GameJoinError {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::None)
    }
}
