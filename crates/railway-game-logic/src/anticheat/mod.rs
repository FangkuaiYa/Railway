//! Anti-cheat validation — pure functions that check for protocol violations.
//!
//! This module provides:
//! - [`CheatResult`]: result enum for individual checks.
//! - [`AnticheatCheck`]: trait for composable checks.
//! - [`validate_all`]: aggregate a batch of checks into a single `GameResult`.
//! - [`checks`]: sub-module with concrete check functions for every scenario.

pub mod checks;

use crate::GameResult;
use crate::error::GameError;

/// Result of a single anti-cheat check.
///
/// `Allow` means the action passed validation.
/// `Cheat` means the action was rejected, with a human-readable reason.
#[derive(Debug, Clone)]
pub enum CheatResult {
    /// The action is permitted.
    Allow,
    /// The action is a protocol violation, with an explanation.
    Cheat { message: String },
}

impl CheatResult {
    /// Returns `true` if this result is [`CheatResult::Allow`].
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow)
    }

    /// Returns `true` if this result is [`CheatResult::Cheat`].
    pub fn is_cheat(&self) -> bool {
        matches!(self, Self::Cheat { .. })
    }

    /// Returns the cheat message if this is a `Cheat` variant, otherwise `None`.
    pub fn cheat_message(&self) -> Option<&str> {
        match self {
            Self::Allow => None,
            Self::Cheat { message } => Some(message.as_str()),
        }
    }
}

/// Trait for composable anti-cheat assertions.
///
/// Implementors return [`CheatResult::Allow`] when the check passes,
/// or [`CheatResult::Cheat`] with a description when it fails.
pub trait AnticheatCheck {
    /// Run the check and return the result.
    fn check(&self) -> CheatResult;
}

/// Run a batch of checks, returning `Ok(())` on all-pass, or
/// `Err(GameError::CheatDetected)` on the first failure.
///
/// # Examples
///
/// ```ignore
/// let results = [
///     check_player_name("Alice"),
///     check_is_host(player.is_host),
/// ];
/// validate_all(&results)?;
/// ```
pub fn validate_all(checks: &[CheatResult]) -> GameResult<()> {
    for result in checks {
        if let CheatResult::Cheat { message } = result {
            return Err(GameError::CheatDetected {
                message: message.clone(),
            });
        }
    }
    Ok(())
}

// Re-export every check function so callers can `use crate::anticheat::*`.
pub use checks::*;
