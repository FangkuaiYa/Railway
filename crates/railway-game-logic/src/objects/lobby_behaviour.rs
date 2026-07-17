//! InnerLobbyBehaviour — a marker object that exists in the lobby scene.
//!
//! In the base game this object controls the lobby UI elements (game settings
//! panel, player list, start-game button, etc.). On a headless server it is
//! a pure placeholder — the server broadcasts its existence to connected
//! clients so their UI can render correctly, but the server itself does not
//! mutate lobby-behaviour state.

use std::sync::Arc;
use async_trait::async_trait;
use railway_hazel::{MessageReader, MessageWriter};
use railway_protocol::{ClientId, NetId, RpcCalls, SpawnFlags};

use crate::objects::InnerNetObject;
use crate::player::ClientPlayer;
use crate::{Game, GameResult};

/// The lobby behaviour object — present while players are in the lobby.
///
/// This object is spawned when the game is created and despawned when the
/// game starts (or when the player moves to the game scene). It is
/// **server-owned** and carries no gameplay state.
pub struct InnerLobbyBehaviour {
    net_id: NetId,
    owner_id: ClientId,
    spawn_flags: SpawnFlags,
    game: Arc<Game>,
}

impl InnerLobbyBehaviour {
    /// Create a new lobby behaviour.
    pub fn new(game: Arc<Game>) -> Self {
        Self {
            net_id: 0,
            owner_id: crate::SERVER_OWNED_ID,
            spawn_flags: SpawnFlags::NONE,
            game,
        }
    }
}

#[async_trait]
impl InnerNetObject for InnerLobbyBehaviour {
    fn net_id(&self) -> NetId {
        self.net_id
    }

    fn owner_id(&self) -> ClientId {
        self.owner_id
    }

    fn spawn_flags(&self) -> SpawnFlags {
        self.spawn_flags
    }

    fn set_net_id(&mut self, net_id: NetId) {
        self.net_id = net_id;
    }

    fn set_owner_id(&mut self, owner_id: ClientId) {
        self.owner_id = owner_id;
    }

    fn set_spawn_flags(&mut self, flags: SpawnFlags) {
        self.spawn_flags = flags;
    }

    async fn serialize(&self, writer: &mut MessageWriter, _initial_state: bool) -> GameResult<()> {
        // Minimal lobby state: write the player count so clients can
        // render the correct number of player slots in the lobby UI.
        let count = self.game.player_count();
        writer.write_packed_u32(count as u32);
        Ok(())
    }

    async fn deserialize(
        &mut self,
        _sender: &ClientPlayer,
        _target: Option<&ClientPlayer>,
        reader: &mut MessageReader,
        _initial_state: bool,
    ) -> GameResult<()> {
        // Deserialization is a no-op for the lobby behaviour.
        // The lobby state is derived from game metadata, not from per-object data.
        // Consume any bytes that arrived so the reader is not corrupted.
        if reader.remaining() > 0 {
            let _count = reader.read_packed_u32();
        }
        Ok(())
    }

    async fn handle_rpc(
        &mut self,
        _sender: &ClientPlayer,
        _target: Option<&ClientPlayer>,
        _call: RpcCalls,
        _reader: &mut MessageReader,
    ) -> GameResult<bool> {
        // LobbyBehaviour does not handle any RPCs. All lobby interactions
        // (start game, change settings, kick player) are handled by the
        // server at the protocol layer, not through object RPCs.
        Ok(false)
    }
}
