//! InnerVoteBanSystem — manages vote-kick functionality.
//!
//! When a player calls a vote to kick someone from the lobby, this object
//! tracks the votes and determines whether the target should be removed.
//!
//! Vote-kick is available in the lobby (pre-game) and requires a majority
//! of non-target players to vote in favor.

use std::sync::Arc;
use async_trait::async_trait;
use dashmap::DashMap;
use railway_hazel::{MessageReader, MessageWriter};
use railway_protocol::{ClientId, NetId, RpcCalls, SpawnFlags};

use crate::error::GameError;
use crate::objects::InnerNetObject;
use crate::player::ClientPlayer;
use crate::{Game, GameResult};

/// Vote-kick system for the lobby phase.
///
/// Players vote to remove a disruptive player before the game starts.
/// Only the host (or a majority) can finalize a kick.
pub struct InnerVoteBanSystem {
    net_id: NetId,
    owner_id: ClientId,
    spawn_flags: SpawnFlags,
    game: Arc<Game>,
    /// Tracks who each voter voted to kick. Keyed by voter's client ID.
    votes: DashMap<ClientId, ClientId>,
}

impl InnerVoteBanSystem {
    /// Create a new vote-ban system.
    pub fn new(game: Arc<Game>) -> Self {
        Self {
            net_id: 0,
            owner_id: crate::SERVER_OWNED_ID,
            spawn_flags: SpawnFlags::NONE,
            game,
            votes: DashMap::new(),
        }
    }

    /// Add a vote from one player to kick another.
    ///
    /// Returns `true` if this is a new vote, `false` if this voter already voted.
    pub fn add_vote(&self, voter_id: ClientId, target_id: ClientId) -> bool {
        // Prevent self-voting (you cannot vote to kick yourself)
        if voter_id == target_id {
            return false;
        }

        // Insert the vote; if the voter already voted, update it.
        let is_new = !self.votes.contains_key(&voter_id);
        self.votes.insert(voter_id, target_id);
        is_new
    }

    /// Remove a player's vote.
    pub fn remove_vote(&self, voter_id: ClientId) {
        self.votes.remove(&voter_id);
    }

    /// Check whether the vote-kick threshold has been met for a given target.
    ///
    /// Returns `true` if enough players voted to kick the target.
    /// The threshold is: more than half of all non-target players.
    pub fn is_kick_threshold_met(&self, target_id: ClientId) -> bool {
        let votes_for_target: usize = self
            .votes
            .iter()
            .filter(|entry| *entry.value() == target_id)
            .count();

        let total_players = self.game.player_count();
        // The target does not get a vote (they cannot vote for themselves).
        // Threshold: > 50% of (total - 1) players.
        let eligible_voters = total_players.saturating_sub(1);

        if eligible_voters == 0 {
            return false;
        }

        votes_for_target > eligible_voters / 2
    }

    /// Process all votes and return the client ID of the player to kick,
    /// if the threshold is met.
    pub fn process_votes(&self) -> Option<ClientId> {
        // Build a tally of vote counts per target.
        let tally: DashMap<ClientId, usize> = DashMap::new();

        for entry in self.votes.iter() {
            let target = *entry.value();
            *tally.entry(target).or_insert(0) += 1;
        }

        let total_players = self.game.player_count();
        let eligible_voters = total_players.saturating_sub(1);
        let threshold = (eligible_voters / 2) + 1;

        // Find a target that meets the threshold.
        for entry in tally.iter() {
            if *entry.value() >= threshold {
                return Some(*entry.key());
            }
        }

        None
    }

    /// Reset all votes (called after a kick succeeds or the game starts).
    pub fn reset(&self) {
        self.votes.clear();
    }

    /// Get the number of votes currently recorded.
    pub fn vote_count(&self) -> usize {
        self.votes.len()
    }
}

#[async_trait]
impl InnerNetObject for InnerVoteBanSystem {
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
        let count = self.vote_count();
        writer.write_packed_u32(count as u32);

        // Serialize each vote: voter u32 (client_id), target u32 (client_id)
        for entry in self.votes.iter() {
            writer.write_i32(*entry.key());
            writer.write_i32(*entry.value());
        }

        Ok(())
    }

    async fn deserialize(
        &mut self,
        _sender: &ClientPlayer,
        _target: Option<&ClientPlayer>,
        reader: &mut MessageReader,
        _initial_state: bool,
    ) -> GameResult<()> {
        let count = reader.read_packed_u32() as usize;

        self.votes.clear();

        for _ in 0..count {
            let voter = reader.read_i32();
            let target = reader.read_i32();
            self.votes.insert(voter, target);
        }

        Ok(())
    }

    async fn handle_rpc(
        &mut self,
        sender: &ClientPlayer,
        _target: Option<&ClientPlayer>,
        call: RpcCalls,
        reader: &mut MessageReader,
    ) -> GameResult<bool> {
        match call {
            RpcCalls::AddVote => {
                // AddVote for VoteBanSystem:
                // The caller (voter) is typically the sender client,
                // and the target is read from the RPC body.
                let target_id = reader.read_i32();

                // Prevent votes during active gameplay.
                let state = self.game.state();
                if !state.can_join() && state != crate::GameState::NotStarted {
                    return Err(GameError::InvalidRpc(
                        "vote-kick is only available in the lobby".into(),
                    ));
                }

                // Prevent self-targeting.
                if target_id == sender.client_id {
                    return Err(GameError::CheatDetected {
                        message: "player cannot vote to kick themselves".into(),
                    });
                }

                // Check that the target is actually in the game.
                if !self.game.players.contains_key(&target_id) {
                    return Err(GameError::PlayerNotFound(target_id));
                }

                self.add_vote(sender.client_id, target_id);

                // After adding the vote, check if the threshold is met.
                // If so, the server should kick the target.
                // (The actual kick is handled by the server layer;
                //  we just report that the vote was processed.)

                Ok(true)
            }

            RpcCalls::ClearVote => {
                // ClearVote for VoteBanSystem: remove the sender's vote.
                self.remove_vote(sender.client_id);
                Ok(true)
            }

            _ => {
                // Unhandled RPC — the caller (server) should relay as-is.
                Ok(false)
            }
        }
    }
}
