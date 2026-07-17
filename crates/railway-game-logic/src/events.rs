//! Game events — emitted by the game logic for plugins and the server to consume.

use railway_protocol::{ClientId, DisconnectReason, GameCode, GameOverReason};

/// Events that occur during a game's lifecycle.
#[derive(Debug, Clone)]
pub enum GameEvent {
    /// A player joined the game.
    PlayerJoined {
        game_code: GameCode,
        client_id: ClientId,
    },

    /// A player left the game.
    PlayerLeft {
        game_code: GameCode,
        client_id: ClientId,
        reason: DisconnectReason,
    },

    /// The game host changed.
    HostChanged {
        game_code: GameCode,
        new_host_id: ClientId,
    },

    /// The game is starting.
    GameStarting {
        game_code: GameCode,
    },

    /// The game has started.
    GameStarted {
        game_code: GameCode,
    },

    /// The game has ended.
    GameEnded {
        game_code: GameCode,
        reason: GameOverReason,
    },

    /// The game was destroyed (all players left).
    GameDestroyed {
        game_code: GameCode,
    },

    /// A meeting was started.
    MeetingStarted {
        game_code: GameCode,
    },

    /// A meeting ended.
    MeetingEnded {
        game_code: GameCode,
    },

    /// A player was murdered.
    PlayerMurdered {
        game_code: GameCode,
        killer_id: ClientId,
        victim_id: ClientId,
    },

    /// A player completed a task.
    TaskCompleted {
        game_code: GameCode,
        client_id: ClientId,
        task_index: u32,
    },

    /// A player was exiled/voted out.
    PlayerExiled {
        game_code: GameCode,
        client_id: ClientId,
    },

    /// A player chatted.
    PlayerChat {
        game_code: GameCode,
        client_id: ClientId,
        message: String,
    },

    /// A player entered a vent.
    PlayerEnterVent {
        game_code: GameCode,
        client_id: ClientId,
        vent_id: u32,
    },

    /// A player exited a vent.
    PlayerExitVent {
        game_code: GameCode,
        client_id: ClientId,
        vent_id: u32,
    },

    /// A sabotage was triggered.
    SabotageTriggered {
        game_code: GameCode,
        system_type: u8,
    },

    /// A sabotage was fixed.
    SabotageFixed {
        game_code: GameCode,
        system_type: u8,
    },

    /// Game privacy was changed.
    GamePrivacyChanged {
        game_code: GameCode,
        is_public: bool,
    },

    /// A player spawned (character created).
    PlayerSpawned {
        game_code: GameCode,
        client_id: ClientId,
        character_net_id: u32,
    },

    /// A player was destroyed (character removed).
    PlayerDestroyed {
        game_code: GameCode,
        client_id: ClientId,
    },
}
