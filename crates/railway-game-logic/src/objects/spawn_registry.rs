//! Spawn registry: maps spawnable object type IDs to factory functions.
//!
//! This mirrors the C# `SpawnableObjects` dictionary.  Factories can be
//! registered at runtime (e.g. during server initialisation or by plugins)
//! and then looked up when a spawn message arrives.
//!
//! # Thread safety
//!
//! The registry is protected by a `parking_lot::RwLock` so that reads
//! (the hot path) do not contend with each other.  Writes (registration)
//! are rare and happen at startup.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use parking_lot::RwLock;

use crate::game::Game;
use crate::objects::InnerNetObject;

/// Type alias for a spawnable-object factory function.
///
/// Takes an `Arc<Game>` and returns a fully initialised `Arc<dyn InnerNetObject>`.
pub type SpawnFactoryFn = Arc<dyn Fn(Arc<Game>) -> Arc<dyn InnerNetObject> + Send + Sync>;

/// Global registry of spawnable object factories, keyed by spawn type ID.
///
/// Use [`register_spawnable`] to add factories and [`create_spawnable`] to
/// instantiate objects from them.
pub static SPAWNABLE_OBJECTS: LazyLock<RwLock<HashMap<u32, SpawnFactoryFn>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

// ── Public API ──────────────────────────────────────────────────────

/// Register a factory function for the given spawn type ID.
///
/// If a factory was already registered for `id` it is replaced and the old
/// factory is returned.
///
/// # Examples
///
/// ```ignore
/// register_spawnable(4, Arc::new(|game| {
///     Arc::new(PlayerControl::new(game))
/// }));
/// ```
pub fn register_spawnable(
    id: u32,
    factory: SpawnFactoryFn,
) -> Option<SpawnFactoryFn> {
    SPAWNABLE_OBJECTS.write().insert(id, factory)
}

/// Create a new InnerNetObject from the spawn type ID.
///
/// Returns `None` when no factory has been registered for `id`.
pub fn create_spawnable(id: u32, game: Arc<Game>) -> Option<Arc<dyn InnerNetObject>> {
    let registry = SPAWNABLE_OBJECTS.read();
    registry.get(&id).map(|factory| factory(game))
}

/// Returns `true` when a factory is registered for the given spawn type ID.
pub fn is_registered(id: u32) -> bool {
    SPAWNABLE_OBJECTS.read().contains_key(&id)
}

/// Returns the number of registered spawnable types.
pub fn registered_count() -> usize {
    SPAWNABLE_OBJECTS.read().len()
}

/// Remove all registered factories.
///
/// This is mainly useful for testing.
pub fn clear_registry() {
    SPAWNABLE_OBJECTS.write().clear();
}

// ── Name / ID helpers (static data, does not depend on registry) ────

/// Returns the human-readable name for a known spawnable object type ID,
/// or `"Unknown"` for unrecognised IDs.
pub fn spawnable_name(object_id: u32) -> &'static str {
    match object_id {
        0 => "SkeldShipStatus",
        1 => "MeetingHud",
        2 => "LobbyBehaviour",
        4 => "PlayerControl",
        5 => "MiraShipStatus",
        6 => "PolusShipStatus",
        7 => "DleksShipStatus",
        8 => "AirshipStatus",
        9 => "HideAndSeekManager",
        10 => "NormalGameManager",
        11 => "PlayerInfo",
        12 => "VoteBanSystem",
        13 => "FungleShipStatus",
        _ => "Unknown",
    }
}

/// Returns the spawnable type ID for a given object name, or `None` if the
/// name is not recognised.
pub fn spawnable_id(name: &str) -> Option<u32> {
    match name {
        "SkeldShipStatus" => Some(0),
        "MeetingHud" => Some(1),
        "LobbyBehaviour" => Some(2),
        "PlayerControl" => Some(4),
        "MiraShipStatus" => Some(5),
        "PolusShipStatus" => Some(6),
        "DleksShipStatus" => Some(7),
        "AirshipStatus" => Some(8),
        "HideAndSeekManager" => Some(9),
        "NormalGameManager" => Some(10),
        "PlayerInfo" => Some(11),
        "VoteBanSystem" => Some(12),
        "FungleShipStatus" => Some(13),
        _ => None,
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc as StdArc;

    /// A minimal InnerNetObject stub used in tests.
    struct StubObject {
        net_id: u32,
        was_called: StdArc<AtomicBool>,
    }

    // We only need a few trait methods for the test; the rest can panic.
    #[async_trait::async_trait]
    impl InnerNetObject for StubObject {
        fn net_id(&self) -> railway_protocol::NetId { self.net_id }
        fn owner_id(&self) -> railway_protocol::ClientId { 0 }
        fn spawn_flags(&self) -> railway_protocol::SpawnFlags {
            railway_protocol::SpawnFlags::NONE
        }
        fn set_net_id(&mut self, id: railway_protocol::NetId) { self.net_id = id; }
        fn set_owner_id(&mut self, _: railway_protocol::ClientId) {}
        fn set_spawn_flags(&mut self, _: railway_protocol::SpawnFlags) {}

        async fn serialize(
            &self,
            _: &mut railway_hazel::MessageWriter,
            _: bool,
        ) -> crate::GameResult<()> { Ok(()) }

        async fn deserialize(
            &mut self,
            _: &crate::player::ClientPlayer,
            _: Option<&crate::player::ClientPlayer>,
            _: &mut railway_hazel::MessageReader,
            _: bool,
        ) -> crate::GameResult<()> { Ok(()) }

        async fn handle_rpc(
            &mut self,
            _: &crate::player::ClientPlayer,
            _: Option<&crate::player::ClientPlayer>,
            _: railway_protocol::RpcCalls,
            _: &mut railway_hazel::MessageReader,
        ) -> crate::GameResult<bool> { Ok(false) }
    }

    fn make_dummy_game() -> Arc<Game> {
        use railway_protocol::game_options::NormalGameOptions;
        use railway_protocol::messages::c2s::host_game::GameFilterOptions;

        Game::new(
            1234,
            Arc::new(NormalGameOptions::default()),
            GameFilterOptions::default(),
        )
    }

    #[test]
    fn register_and_create() {
        clear_registry();

        let called = StdArc::new(AtomicBool::new(false));
        let called2 = StdArc::clone(&called);

        let factory: SpawnFactoryFn = Arc::new(move |_game| {
            called2.store(true, Ordering::SeqCst);
            Arc::new(StubObject {
                net_id: 0,
                was_called: StdArc::clone(&called2),
            })
        });

        assert!(register_spawnable(99, factory).is_none());
        assert!(is_registered(99));
        assert_eq!(registered_count(), 1);

        let game = make_dummy_game();
        let obj = create_spawnable(99, game).expect("factory should exist");
        assert_eq!(obj.net_id(), 0);
        assert!(called.load(Ordering::SeqCst));
    }

    #[test]
    fn create_unknown_id_returns_none() {
        clear_registry();
        let game = make_dummy_game();
        assert!(create_spawnable(99999, game).is_none());
    }

    #[test]
    fn replace_existing_factory() {
        clear_registry();

        let f1: SpawnFactoryFn = Arc::new(|_game| {
            Arc::new(StubObject { net_id: 1, was_called: StdArc::new(AtomicBool::new(false)) })
        });
        let f2: SpawnFactoryFn = Arc::new(|_game| {
            Arc::new(StubObject { net_id: 2, was_called: StdArc::new(AtomicBool::new(false)) })
        });

        let old = register_spawnable(42, f1);
        assert!(old.is_none());

        let old = register_spawnable(42, f2);
        assert!(old.is_some());

        let game = make_dummy_game();
        let obj = create_spawnable(42, game).expect("factory should exist");
        assert_eq!(obj.net_id(), 2); // second factory wins
    }

    #[test]
    fn clear_removes_all() {
        clear_registry();

        let f: SpawnFactoryFn = Arc::new(|_game| {
            Arc::new(StubObject { net_id: 0, was_called: StdArc::new(AtomicBool::new(false)) })
        });
        register_spawnable(1, f.clone());
        register_spawnable(2, f);
        assert_eq!(registered_count(), 2);

        clear_registry();
        assert_eq!(registered_count(), 0);
    }

    #[test]
    fn spawnable_name_lookup() {
        assert_eq!(spawnable_name(0), "SkeldShipStatus");
        assert_eq!(spawnable_name(4), "PlayerControl");
        assert_eq!(spawnable_name(13), "FungleShipStatus");
        assert_eq!(spawnable_name(255), "Unknown");
    }

    #[test]
    fn spawnable_id_lookup() {
        assert_eq!(spawnable_id("MeetingHud"), Some(1));
        assert_eq!(spawnable_id("PlayerControl"), Some(4));
        assert_eq!(spawnable_id("NonExistent"), None);
    }
}
