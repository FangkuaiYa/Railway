//! Game objects: InnerNetObject trait and its implementations.

mod inner_net_object;
pub mod game_data;
pub mod game_manager_obj;
pub mod lobby_behaviour;
pub mod meeting_hud;
pub mod network_transform;
pub mod player_control;
pub mod player_physics;
pub mod ship_status;
pub mod spawn_registry;
pub mod vote_ban_system;

pub use inner_net_object::InnerNetObject;
pub use game_data::{InnerGameData, PlayerInfo};
pub use game_manager_obj::{InnerHideAndSeekManager, InnerNormalGameManager};
pub use lobby_behaviour::InnerLobbyBehaviour;
pub use meeting_hud::InnerMeetingHud;
pub use network_transform::InnerCustomNetworkTransform;
pub use player_control::InnerPlayerControl;
pub use player_physics::InnerPlayerPhysics;
pub use ship_status::{
    InnerAirshipStatus, InnerDleksShipStatus, InnerFungleShipStatus,
    InnerMiraShipStatus, InnerPolusShipStatus, InnerShipStatus, InnerSkeldShipStatus,
    ShipSystem, SystemTypes,
};
pub use vote_ban_system::InnerVoteBanSystem;

/// Register the built-in spawnable object factories.
///
/// **Must be called once at server startup** (before any client can send a
/// SpawnFlag message) — before this was added, `SPAWNABLE_OBJECTS` was
/// always empty and `create_spawnable` always returned `None`, so the
/// server never tracked ANY networked object, ever. That meant every
/// `DataFlag`/`RpcFlag` lookup by NetId silently failed (object not
/// found), even though the wire protocol itself was fine.
pub fn register_default_spawnables() {
    use crate::Game;
    use spawn_registry::register_spawnable;

    register_spawnable(0, Arc::new(|game: Arc<Game>| {
        Arc::new(ship_status::InnerSkeldShipStatus::new(game)) as Arc<dyn InnerNetObject>
    }));
    register_spawnable(1, Arc::new(|game: Arc<Game>| {
        Arc::new(InnerMeetingHud::new(game)) as Arc<dyn InnerNetObject>
    }));
    register_spawnable(2, Arc::new(|game: Arc<Game>| {
        Arc::new(InnerLobbyBehaviour::new(game)) as Arc<dyn InnerNetObject>
    }));
    register_spawnable(4, Arc::new(|game: Arc<Game>| {
        Arc::new(InnerPlayerControl::new(game)) as Arc<dyn InnerNetObject>
    }));
    register_spawnable(5, Arc::new(|game: Arc<Game>| {
        Arc::new(ship_status::InnerMiraShipStatus::new(game)) as Arc<dyn InnerNetObject>
    }));
    register_spawnable(6, Arc::new(|game: Arc<Game>| {
        Arc::new(ship_status::InnerPolusShipStatus::new(game)) as Arc<dyn InnerNetObject>
    }));
    register_spawnable(7, Arc::new(|game: Arc<Game>| {
        Arc::new(ship_status::InnerDleksShipStatus::new(game)) as Arc<dyn InnerNetObject>
    }));
    register_spawnable(8, Arc::new(|game: Arc<Game>| {
        Arc::new(ship_status::InnerAirshipStatus::new(game)) as Arc<dyn InnerNetObject>
    }));
    register_spawnable(9, Arc::new(|game: Arc<Game>| {
        Arc::new(InnerHideAndSeekManager::new(game)) as Arc<dyn InnerNetObject>
    }));
    register_spawnable(10, Arc::new(|game: Arc<Game>| {
        Arc::new(InnerNormalGameManager::new(game)) as Arc<dyn InnerNetObject>
    }));
    register_spawnable(12, Arc::new(|game: Arc<Game>| {
        Arc::new(InnerVoteBanSystem::new(game)) as Arc<dyn InnerNetObject>
    }));
    register_spawnable(13, Arc::new(|game: Arc<Game>| {
        Arc::new(ship_status::InnerFungleShipStatus::new(game)) as Arc<dyn InnerNetObject>
    }));
}

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
// NetId, ClientId etc. from railway-protocol

/// The type registry maps spawnable object IDs to factory functions.
pub type SpawnFactory = fn(game: Arc<crate::Game>) -> Arc<dyn InnerNetObject>;

/// Global registry for spawnable game objects.
pub static SPAWN_REGISTRY: LazyLock<HashMap<u32, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert(0, "SkeldShipStatus");
    m.insert(1, "MeetingHud");
    m.insert(2, "LobbyBehaviour");
    m.insert(4, "PlayerControl");
    m.insert(5, "MiraShipStatus");
    m.insert(6, "PolusShipStatus");
    m.insert(7, "DleksShipStatus");
    m.insert(8, "AirshipStatus");
    m.insert(9, "HideAndSeekManager");
    m.insert(10, "NormalGameManager");
    m.insert(11, "PlayerInfo");
    m.insert(12, "VoteBanSystem");
    m.insert(13, "FungleShipStatus");
    m
});
