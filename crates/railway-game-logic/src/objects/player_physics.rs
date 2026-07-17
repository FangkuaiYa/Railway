//! InnerPlayerPhysics — the physical representation of a player character.
//!
//! Tracks position and velocity. Position is replicated to all clients;
//! velocity is also replicated for client-side prediction.

use std::sync::Arc;
use async_trait::async_trait;
use railway_hazel::{MessageReader, MessageWriter};
use railway_protocol::{ClientId, NetId, RpcCalls, SpawnFlags};
use parking_lot::Mutex;

use crate::objects::InnerNetObject;
use crate::player::ClientPlayer;
use crate::{Game, GameResult};

/// Physics state for a player character.
///
/// Owned by InnerPlayerControl as a child component. The owner client
/// sends position/velocity updates; the server validates and broadcasts them.
pub struct InnerPlayerPhysics {
    net_id: NetId,
    owner_id: ClientId,
    spawn_flags: SpawnFlags,
    #[allow(dead_code)]
    game: Arc<Game>,
    position_x: Mutex<f32>,
    position_y: Mutex<f32>,
    velocity_x: Mutex<f32>,
    velocity_y: Mutex<f32>,
    parent_player_control_net_id: Mutex<Option<NetId>>,
}

impl InnerPlayerPhysics {
    /// Create a new physics component.
    pub fn new(game: Arc<Game>) -> Self {
        Self {
            net_id: 0,
            owner_id: crate::SERVER_OWNED_ID,
            spawn_flags: SpawnFlags::IS_CLIENT_CHARACTER,
            game,
            position_x: Mutex::new(0.0),
            position_y: Mutex::new(0.0),
            velocity_x: Mutex::new(0.0),
            velocity_y: Mutex::new(0.0),
            parent_player_control_net_id: Mutex::new(None),
        }
    }

    /// Set the position (typically called by the owning client or server authority).
    pub fn set_position(&self, x: f32, y: f32) {
        let mut px = self.position_x.lock();
        let mut py = self.position_y.lock();
        *px = x;
        *py = y;
    }

    /// Get the current position.
    pub fn get_position(&self) -> (f32, f32) {
        let px = self.position_x.lock();
        let py = self.position_y.lock();
        (*px, *py)
    }

    /// Set the velocity.
    pub fn set_velocity(&self, vx: f32, vy: f32) {
        let mut vx_guard = self.velocity_x.lock();
        let mut vy_guard = self.velocity_y.lock();
        *vx_guard = vx;
        *vy_guard = vy;
    }

    /// Get the current velocity.
    pub fn get_velocity(&self) -> (f32, f32) {
        let vx = self.velocity_x.lock();
        let vy = self.velocity_y.lock();
        (*vx, *vy)
    }

    /// Get the parent player control net ID.
    pub fn parent_player_control_net_id(&self) -> Option<NetId> {
        *self.parent_player_control_net_id.lock()
    }

    /// Set the parent player control net ID.
    pub fn set_parent_player_control_net_id(&self, id: Option<NetId>) {
        *self.parent_player_control_net_id.lock() = id;
    }
}

#[async_trait]
impl InnerNetObject for InnerPlayerPhysics {
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
        let (px, py) = self.get_position();
        let (vx, vy) = self.get_velocity();

        writer.write_f32(px);
        writer.write_f32(py);
        writer.write_f32(vx);
        writer.write_f32(vy);

        Ok(())
    }

    async fn deserialize(
        &mut self,
        sender: &ClientPlayer,
        _target: Option<&ClientPlayer>,
        reader: &mut MessageReader,
        _initial_state: bool,
    ) -> GameResult<()> {
        let pos_x = reader.read_f32();
        let pos_y = reader.read_f32();
        let vel_x = reader.read_f32();
        let vel_y = reader.read_f32();

        // Only update state if the sender owns this object.
        if sender.client_id == self.owner_id {
            self.set_position(pos_x, pos_y);
            self.set_velocity(vel_x, vel_y);
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
        // PlayerPhysics does not handle any direct RPCs — all movement is handled
        // by InnerCustomNetworkTransform (SnapTo) or InnerPlayerControl (EnterVent, etc.).
        Ok(false)
    }
}
