//! Game state machine.

/// The lifecycle states of an Among Us game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameState {
    /// Game has been created, waiting for players.
    NotStarted,

    /// Game is starting (host pressed Start, countdown in progress).
    Starting,

    /// Game is in progress.
    Started,

    /// Game has ended (meeting concluded or sabotage victory).
    Ended,

    /// Game is being destroyed / cleaned up.
    Destroyed,
}

impl GameState {
    /// Returns true if players can join the game.
    pub fn can_join(self) -> bool {
        matches!(self, Self::NotStarted)
    }

    /// Returns true if gameplay is active.
    pub fn is_playing(self) -> bool {
        matches!(self, Self::Started)
    }

    /// Returns true if the game is still alive (not destroyed).
    pub fn is_alive(self) -> bool {
        !matches!(self, Self::Destroyed)
    }
}
