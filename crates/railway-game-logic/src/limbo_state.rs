//! Limbo states for players in a game.
//!
//! Limbo states control what messages a player receives and whether
//! they can interact with the game world.

use bitflags::bitflags;

bitflags! {
    /// Player limbo state flags (can be combined).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct LimboStates: u8 {
        /// Player is in the game and receiving all messages.
        const NotLimbo = 1;

        /// Player joined but hasn't spawned yet.
        const PreSpawn = 2;

        /// Player is waiting for the host to rejoin.
        const WaitingForHost = 4;
    }
}

impl Default for LimboStates {
    fn default() -> Self {
        Self::PreSpawn
    }
}
