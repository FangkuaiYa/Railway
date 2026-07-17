//! InnerNetObject trait — the base trait for all networked game objects.

use std::sync::Arc;
use async_trait::async_trait;
use railway_hazel::{MessageReader, MessageWriter};
use railway_protocol::{ClientId, NetId, RpcCalls, SpawnFlags};

use crate::player::ClientPlayer;
use crate::{GameResult, SERVER_OWNED_ID};

/// Trait for objects that can be serialized/deserialized and handle RPCs.
///
/// All game objects (PlayerControl, ShipStatus, MeetingHud, etc.) implement this.
#[async_trait]
pub trait InnerNetObject: Send + Sync {
    /// The unique network ID of this object.
    fn net_id(&self) -> NetId;

    /// The client ID that owns this object.
    fn owner_id(&self) -> ClientId;

    /// Spawn flags for this object.
    fn spawn_flags(&self) -> SpawnFlags;

    /// Set the network ID.
    fn set_net_id(&mut self, net_id: NetId);

    /// Set the owner ID.
    fn set_owner_id(&mut self, owner_id: ClientId);

    /// Set spawn flags.
    fn set_spawn_flags(&mut self, flags: SpawnFlags);

    /// Serialize this object's state into a message writer.
    ///
    /// `initial_state` is true when this is the first serialization (spawn).
    async fn serialize(&self, writer: &mut MessageWriter, initial_state: bool) -> GameResult<()>;

    /// Deserialize this object's state from a message reader.
    ///
    /// `initial_state` is true when this is the first deserialization (spawn).
    async fn deserialize(
        &mut self,
        sender: &ClientPlayer,
        target: Option<&ClientPlayer>,
        reader: &mut MessageReader,
        initial_state: bool,
    ) -> GameResult<()>;

    /// Handle an incoming RPC call on this object.
    ///
    /// Returns `Ok(true)` if the RPC was handled, `Ok(false)` if not handled,
    /// or `Err(...)` if there was an error.
    async fn handle_rpc(
        &mut self,
        sender: &ClientPlayer,
        target: Option<&ClientPlayer>,
        call: RpcCalls,
        reader: &mut MessageReader,
    ) -> GameResult<bool>;

    /// Called after this object is successfully spawned.
    async fn on_spawn(&self) -> GameResult<()> {
        Ok(())
    }

    /// Returns the list of child components (for InnerNetObject hierarchy).
    fn components(&self) -> &[Arc<dyn InnerNetObject>] {
        &[]
    }

    // ── Helper methods ──────────────────────────────────────────────

    /// Returns true if this object is owned by the server.
    ///
    /// Server-owned objects (like ShipStatus, MeetingHud) are managed
    /// by the game itself rather than any particular player.
    fn is_server_owned(&self) -> bool {
        self.owner_id() == SERVER_OWNED_ID
    }

    /// Returns the number of child components attached to this object.
    ///
    /// This is a convenience method equivalent to `components().len()`.
    fn component_count(&self) -> usize {
        self.components().len()
    }

    /// Returns a reference to the component at the given index, or `None` if
    /// the index is out of bounds.
    ///
    /// Components are stored in-order as returned by [`components()`].
    fn get_component_index(&self, index: usize) -> Option<&dyn InnerNetObject> {
        self.components().get(index).map(|arc| arc.as_ref())
    }
}
