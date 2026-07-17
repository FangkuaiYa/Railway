//! InnerCustomNetworkTransform — authoritative networked transform.
//!
//! Handles position snapping, sequence-ordered updates, and zipline interactions.
//! This is the main positional authority for player characters, separate from
//! InnerPlayerPhysics which handles the raw physics state.

use std::sync::Arc;
use async_trait::async_trait;
use railway_hazel::{MessageReader, MessageWriter};
use railway_protocol::{ClientId, NetId, RpcCalls, SpawnFlags};
use parking_lot::Mutex;

use crate::objects::InnerNetObject;
use crate::player::ClientPlayer;
use crate::{Game, GameResult};

/// Networked transform with snapshot interpolation support.
///
/// Maintains a sequence ID so the server and other clients can determine
/// which position update is the most recent.
pub struct InnerCustomNetworkTransform {
    net_id: NetId,
    owner_id: ClientId,
    spawn_flags: SpawnFlags,
    #[allow(dead_code)]
    game: Arc<Game>,
    last_sequence_id: Mutex<u16>,
    target_position_x: Mutex<f32>,
    target_position_y: Mutex<f32>,
    prev_position_x: Mutex<f32>,
    prev_position_y: Mutex<f32>,
}

impl InnerCustomNetworkTransform {
    /// Create a new network transform.
    pub fn new(game: Arc<Game>) -> Self {
        Self {
            net_id: 0,
            owner_id: crate::SERVER_OWNED_ID,
            spawn_flags: SpawnFlags::IS_CLIENT_CHARACTER,
            game,
            last_sequence_id: Mutex::new(0),
            target_position_x: Mutex::new(0.0),
            target_position_y: Mutex::new(0.0),
            prev_position_x: Mutex::new(0.0),
            prev_position_y: Mutex::new(0.0),
        }
    }

    /// Get the current target position (where the character is moving to).
    pub fn get_position(&self) -> (f32, f32) {
        let tx = self.target_position_x.lock();
        let ty = self.target_position_y.lock();
        (*tx, *ty)
    }

    /// Get the previous position (for interpolation).
    pub fn get_prev_position(&self) -> (f32, f32) {
        let px = self.prev_position_x.lock();
        let py = self.prev_position_y.lock();
        (*px, *py)
    }

    /// Set the target position (where the character should move to).
    pub fn set_target_position(&self, x: f32, y: f32) {
        let mut tx = self.target_position_x.lock();
        let mut ty = self.target_position_y.lock();
        *tx = x;
        *ty = y;
    }

    /// Set the previous position (for interpolation — the position at the start
    /// of the current movement).
    pub fn set_prev_position(&self, x: f32, y: f32) {
        let mut px = self.prev_position_x.lock();
        let mut py = self.prev_position_y.lock();
        *px = x;
        *py = y;
    }

    /// Get the last sequence ID.
    pub fn last_sequence_id(&self) -> u16 {
        *self.last_sequence_id.lock()
    }

    /// Set the last sequence ID.
    pub fn set_last_sequence_id(&self, id: u16) {
        *self.last_sequence_id.lock() = id;
    }

    /// Increment and return the next sequence ID.
    pub fn next_sequence_id(&self) -> u16 {
        let mut id = self.last_sequence_id.lock();
        *id = id.wrapping_add(1);
        *id
    }
}

#[async_trait]
impl InnerNetObject for InnerCustomNetworkTransform {
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
        let seq_id = self.last_sequence_id();
        let (tx, ty) = self.get_position();
        let (px, py) = self.get_prev_position();

        writer.write_u16(seq_id);
        writer.write_f32(tx);
        writer.write_f32(ty);
        writer.write_f32(px);
        writer.write_f32(py);

        Ok(())
    }

    async fn deserialize(
        &mut self,
        sender: &ClientPlayer,
        _target: Option<&ClientPlayer>,
        reader: &mut MessageReader,
        _initial_state: bool,
    ) -> GameResult<()> {
        let seq_id = reader.read_u16();
        let target_x = reader.read_f32();
        let target_y = reader.read_f32();
        let prev_x = reader.read_f32();
        let prev_y = reader.read_f32();

        // Only update state if the sender owns this object.
        if sender.client_id != self.owner_id {
            return Ok(());
        }

        let current_seq = self.last_sequence_id();

        // Track whether this is a newer sequence. The sequence wraps at u16::MAX,
        // so we use wrapping arithmetic to compare — interpret the distance forwards
        // modulo 2^16. If the next n within 32767 steps equals our current ID,
        // then the offered seq IS the newer one.
        let delta = seq_id.wrapping_sub(current_seq) as i16;
        let is_newer = delta > 0;

        if is_newer {
            self.set_last_sequence_id(seq_id);
            self.set_target_position(target_x, target_y);
            self.set_prev_position(prev_x, prev_y);
        }

        Ok(())
    }

    async fn handle_rpc(
        &mut self,
        _sender: &ClientPlayer,
        _target: Option<&ClientPlayer>,
        call: RpcCalls,
        reader: &mut MessageReader,
    ) -> GameResult<bool> {
        match call {
            RpcCalls::SnapTo => {
                // SnapTo RPC format: x (f32), y (f32), optionally prev_x (f32), prev_y (f32).
                // The last 8 bytes are present on newer versions.
                let snap_x = reader.read_f32();
                let snap_y = reader.read_f32();

                self.set_target_position(snap_x, snap_y);

                // After position, see if there are remaining bytes for prev position
                // (the C# implementation checks for 8 additional bytes = 2 f32s).
                // We consume them if present; some protocol versions omit them.
                if reader.remaining() >= 8 {
                    let prev_x = reader.read_f32();
                    let prev_y = reader.read_f32();
                    self.set_prev_position(prev_x, prev_y);
                } else {
                    // When prev position is not sent, the current position
                    // was the previous position (instant snap).
                    self.set_prev_position(snap_x, snap_y);
                }

                // Increment sequence so observers know this is a new authoritative position.
                self.next_sequence_id();

                Ok(true)
            }

            RpcCalls::CheckZipline => {
                // CheckZipline: the client asks the server whether it can use a zipline.
                // This typically comes from a non-owner checking if a zipline is available.
                // The server should validate and respond.
                //
                // The RPC body is empty for CheckZipline — it is a query.
                // For now, we accept all zipline queries and let the client proceed.
                Ok(true)
            }

            RpcCalls::UseZipline => {
                // UseZipline: the client reports that it has started using a zipline.
                // The server validates and broadcasts the position changes.
                //
                // C# format: UseZipline does not carry parameters in the RPC body;
                // the position changes come through regular transform updates.
                // We mark this as handled so the server broadcasts it.
                Ok(true)
            }

            _ => {
                // Unknown RPC — not handled by NetworkTransform.
                Ok(false)
            }
        }
    }
}
