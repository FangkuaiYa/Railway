//! InnerFungleShipStatus — ship status for the Fungle map.
//!
//! The Fungle has the following systems:
//! Reactor, Electrical, O2, Security, Doors, Comms, Laboratory, MushroomMixup, HudOverride

use std::sync::Arc;

use async_trait::async_trait;
use railway_hazel::{MessageReader, MessageWriter};
use railway_protocol::{ClientId, MapType, NetId, RpcCalls, SpawnFlags};

use crate::objects::InnerNetObject;
use crate::objects::ship_status::ship_status::{InnerShipStatus, SystemTypes};
use crate::player::ClientPlayer;
use crate::{Game, GameResult};

/// Ship status for the Fungle.
pub struct InnerFungleShipStatus {
    pub inner: InnerShipStatus,
}

impl InnerFungleShipStatus {
    /// Systems present on the Fungle.
    pub const SYSTEM_TYPES: &'static [SystemTypes] = &[
        SystemTypes::Reactor,
        SystemTypes::Electrical,
        SystemTypes::O2,
        SystemTypes::Security,
        SystemTypes::Doors,
        SystemTypes::Comms,
        SystemTypes::Laboratory,
        SystemTypes::MushroomMixup,
        SystemTypes::HudOverride,
    ];

    /// Create a new Fungle ship status.
    pub fn new(game: Arc<Game>) -> Self {
        Self {
            inner: InnerShipStatus::new(game, MapType::Fungle),
        }
    }
}

#[async_trait]
impl InnerNetObject for InnerFungleShipStatus {
    fn net_id(&self) -> NetId {
        self.inner.net_id()
    }

    fn owner_id(&self) -> ClientId {
        self.inner.owner_id()
    }

    fn spawn_flags(&self) -> SpawnFlags {
        self.inner.spawn_flags()
    }

    fn set_net_id(&mut self, id: NetId) {
        self.inner.set_net_id(id);
    }

    fn set_owner_id(&mut self, id: ClientId) {
        self.inner.set_owner_id(id);
    }

    fn set_spawn_flags(&mut self, flags: SpawnFlags) {
        self.inner.set_spawn_flags(flags);
    }

    async fn serialize(&self, writer: &mut MessageWriter, initial_state: bool) -> GameResult<()> {
        self.inner.serialize(writer, initial_state).await
    }

    async fn deserialize(
        &mut self,
        sender: &ClientPlayer,
        target: Option<&ClientPlayer>,
        reader: &mut MessageReader,
        initial_state: bool,
    ) -> GameResult<()> {
        self.inner.deserialize(sender, target, reader, initial_state).await
    }

    async fn handle_rpc(
        &mut self,
        sender: &ClientPlayer,
        target: Option<&ClientPlayer>,
        call: RpcCalls,
        reader: &mut MessageReader,
    ) -> GameResult<bool> {
        self.inner.handle_rpc(sender, target, call, reader).await
    }
}
