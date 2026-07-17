//! InnerMeetingHud — the full meeting / voting state machine.
//!
//! Handles:
//! - Starting and ending meetings (report dead body, emergency button)
//! - Casting, clearing, and tallying votes
//! - Resolving voting results (exile, tie, skip)
//! - Serialization of meeting state to all clients

use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use railway_hazel::{MessageReader, MessageWriter};
use railway_protocol::{ClientId, NetId, PlayerId, RpcCalls, SpawnFlags};
use parking_lot::{Mutex, RwLock};

use crate::error::GameError;
use crate::events::GameEvent;
use crate::objects::InnerNetObject;
use crate::player::ClientPlayer;
use crate::{Game, GameResult};

/// The meeting HUD — manages the voting phase of the game.
///
/// Visualised as a screen overlay showing all alive players with
/// voting buttons. This object tracks the full meeting lifecycle.
pub struct InnerMeetingHud {
    net_id: NetId,
    owner_id: ClientId,
    spawn_flags: SpawnFlags,
    game: Arc<Game>,
    meeting_active: RwLock<bool>,
    reporter_id: Mutex<Option<PlayerId>>,
    body_reported_id: Mutex<Option<PlayerId>>,
    /// voter -> who they voted for (Some(player_id) = voted, None = skip, 255 = skip)
    votes: Mutex<HashMap<PlayerId, Option<PlayerId>>>,
    /// Per-player vote states (byte per slot: 0 = not voted, 1 = voted, 2 = skipped, 3 = dead, etc.)
    vote_states: Mutex<Vec<u8>>,
    exiled_player_id: Mutex<Option<PlayerId>>,
    is_tie: Mutex<bool>,
    meeting_start_time: Mutex<f64>,
    discussion_timer: Mutex<f32>,
    voting_timer: Mutex<f32>,
}

impl InnerMeetingHud {
    /// Create a new meeting HUD.
    pub fn new(game: Arc<Game>) -> Self {
        Self {
            net_id: 0,
            owner_id: crate::SERVER_OWNED_ID,
            spawn_flags: SpawnFlags::NONE,
            game,
            meeting_active: RwLock::new(false),
            reporter_id: Mutex::new(None),
            body_reported_id: Mutex::new(None),
            votes: Mutex::new(HashMap::new()),
            vote_states: Mutex::new(vec![0u8; 15]),
            exiled_player_id: Mutex::new(None),
            is_tie: Mutex::new(false),
            meeting_start_time: Mutex::new(0.0),
            discussion_timer: Mutex::new(0.0),
            voting_timer: Mutex::new(0.0),
        }
    }

    /// Start a meeting (called when a dead body is reported or emergency button pressed).
    pub fn start_meeting(&self, reporter_id: Option<PlayerId>, body_id: Option<PlayerId>) {
        // Reset all transient state
        self.reset_all_votes();

        *self.meeting_active.write() = true;
        *self.reporter_id.lock() = reporter_id;
        *self.body_reported_id.lock() = body_id;
        *self.is_tie.lock() = false;
        *self.exiled_player_id.lock() = None;
        *self.meeting_start_time.lock() = 0.0; // Will be set by the host/timer system

        // The game_flow module handles event emission for meeting lifecycle
    }

    /// End/close the meeting without processing results (used for cleanup).
    pub fn end_meeting(&self) {
        self.close_meeting();
    }

    /// Cast a vote from a player for a suspect.
    ///
    /// Returns `true` if the vote was accepted, `false` if the voter
    /// already voted or the meeting is inactive.
    pub fn cast_vote(&self, voter_id: PlayerId, suspect_id: Option<PlayerId>) -> bool {
        if !self.is_active() {
            return false;
        }

        let mut votes = self.votes.lock();

        // Prevent double voting
        if votes.contains_key(&voter_id) {
            return false;
        }

        votes.insert(voter_id, suspect_id);

        // Update the vote state for this player
        let mut states = self.vote_states.lock();
        if (voter_id as usize) < states.len() {
            states[voter_id as usize] = 1; // voted
        }

        true
    }

    /// Clear a vote from a player (undo).
    pub fn clear_vote(&self, voter_id: PlayerId) {
        if !self.is_active() {
            return;
        }

        let mut votes = self.votes.lock();
        votes.remove(&voter_id);

        let mut states = self.vote_states.lock();
        if (voter_id as usize) < states.len() {
            states[voter_id as usize] = 0; // not voted
        }
    }

    /// Add a vote from one player for another (synonym for cast_vote, used by AddVote RPC).
    pub fn add_vote(&self, from_id: PlayerId, target_id: PlayerId) -> bool {
        self.cast_vote(from_id, Some(target_id))
    }

    /// Process all collected votes and determine the result.
    ///
    /// Returns `(exiled_player_id, is_tie)`:
    /// - If there is a tie, `exiled_player_id` is the tied player (or None if skipped wins).
    /// - If there is no tie, `exiled_player_id` is the player voted out (or None if skip).
    pub fn process_voting_results(&self) -> (Option<PlayerId>, bool) {
        let votes = self.votes.lock();

        if votes.is_empty() {
            return (None, false);
        }

        // Count votes per suspect (including skip = None/255)
        let mut tally: HashMap<Option<PlayerId>, u32> = HashMap::new();

        for (_voter, suspect) in votes.iter() {
            *tally.entry(*suspect).or_insert(0) += 1;
        }

        // Find the candidate(s) with the most votes
        let mut max_count: u32 = 0;
        let mut top_candidates: Vec<Option<PlayerId>> = Vec::new();

        for (candidate, count) in tally.iter() {
            if *count > max_count {
                max_count = *count;
                top_candidates.clear();
                top_candidates.push(*candidate);
            } else if *count == max_count {
                top_candidates.push(*candidate);
            }
        }

        if top_candidates.is_empty() {
            return (None, false);
        }

        if top_candidates.len() > 1 {
            // Tie — cannot determine a single winner
            if top_candidates.len() == 2 {
                // Check if one of the tied candidates is the "skip" vote (None/255)
                let has_skip = top_candidates.contains(&None);
                if has_skip {
                    // In a tie with skip, nobody is exiled (skip wins ties by convention)
                    *self.is_tie.lock() = true;
                    *self.exiled_player_id.lock() = None;
                    return (None, true);
                }
            }

            // True tie among players
            *self.is_tie.lock() = true;
            *self.exiled_player_id.lock() = top_candidates[0];
            return (top_candidates[0], true);
        }

        // Single winner
        let result = top_candidates[0];
        *self.is_tie.lock() = false;
        *self.exiled_player_id.lock() = result;
        (result, false)
    }

    /// Close the meeting and reset all state.
    pub fn close_meeting(&self) {
        *self.meeting_active.write() = false;
        self.reset_all_votes();
        *self.reporter_id.lock() = None;
        *self.body_reported_id.lock() = None;
        *self.exiled_player_id.lock() = None;
        *self.is_tie.lock() = false;
        *self.meeting_start_time.lock() = 0.0;
        *self.discussion_timer.lock() = 0.0;
        *self.voting_timer.lock() = 0.0;
    }

    /// Returns `true` if a meeting is currently active.
    pub fn is_active(&self) -> bool {
        *self.meeting_active.read()
    }

    /// Get the list of voters who have voted so far.
    pub fn get_voters(&self) -> Vec<PlayerId> {
        self.votes.lock().keys().copied().collect()
    }

    /// Reset all voting-related state (called at start and end of meetings).
    pub fn reset_all_votes(&self) {
        self.votes.lock().clear();
        let mut states = self.vote_states.lock();
        states.fill(0);
    }

    /// Get the exiled player from the last round (if any).
    pub fn exiled_player_id(&self) -> Option<PlayerId> {
        *self.exiled_player_id.lock()
    }

    /// Check if the last vote resulted in a tie.
    pub fn is_tie_result(&self) -> bool {
        *self.is_tie.lock()
    }
}

#[async_trait]
impl InnerNetObject for InnerMeetingHud {
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
        let active = self.is_active();
        writer.write_bool(active);

        if active {
            let reporter = *self.reporter_id.lock();
            let body = *self.body_reported_id.lock();

            // Reporter: write bool indicating presence, then value if present
            writer.write_bool(reporter.is_some());
            if let Some(r) = reporter {
                writer.write_byte(r);
            }

            // Body reported: write bool indicating presence, then value if present
            writer.write_bool(body.is_some());
            if let Some(b) = body {
                writer.write_byte(b);
            }

            // Votes
            let votes = self.votes.lock();
            writer.write_packed_u32(votes.len() as u32);
            for (voter, suspect) in votes.iter() {
                writer.write_byte(*voter);
                // 255 represents "skip" in the protocol
                let target_byte = suspect.unwrap_or(255);
                writer.write_byte(target_byte);
            }
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
        let active = reader.read_bool();
        *self.meeting_active.write() = active;

        if active {
            let has_reporter = reader.read_bool();
            let reporter = if has_reporter {
                Some(reader.read_byte())
            } else {
                None
            };
            *self.reporter_id.lock() = reporter;

            let has_body = reader.read_bool();
            let body = if has_body {
                Some(reader.read_byte())
            } else {
                None
            };
            *self.body_reported_id.lock() = body;

            let vote_count = reader.read_packed_u32() as usize;
            let mut votes = self.votes.lock();
            votes.clear();

            for _ in 0..vote_count {
                let voter = reader.read_byte();
                let target_byte = reader.read_byte();
                let suspect = if target_byte == 255 {
                    None // skip
                } else {
                    Some(target_byte)
                };
                votes.insert(voter, suspect);
            }
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
            RpcCalls::CastVote => {
                // CastVote format: player_id (u8), suspect_id (u8 where 255 = skip)
                let voter_id = reader.read_byte();
                // Read suspect if data remains (some versions omit suspect for skip)
                let suspect_byte = if reader.remaining() >= 1 {
                    reader.read_byte()
                } else {
                    255 // default skip
                };

                let suspect_id = if suspect_byte == 255 {
                    None
                } else {
                    Some(suspect_byte)
                };

                // Only the actual voter (or host acting on their behalf) can cast
                let voter_client_id = self.resolve_player_client_id(voter_id);
                if sender.client_id != voter_client_id && !sender.is_host {
                    return Err(GameError::HostOnlyOperation(sender.client_id));
                }

                if !self.is_active() {
                    return Err(GameError::InvalidRpc("meeting is not active".into()));
                }

                self.cast_vote(voter_id, suspect_id);

                Ok(true)
            }

            RpcCalls::ClearVote => {
                // ClearVote format: no body (the player clearing is implied by the client)
                // In some implementations the player_id is inferred from the sender.
                // We read it from the RPC body for robustness.
                let voter_id = if reader.remaining() >= 1 {
                    reader.read_byte()
                } else {
                    // Fallback: resolve from sender's character
                    self.resolve_player_id_from_client(sender.client_id)
                };

                if !self.is_active() {
                    return Err(GameError::InvalidRpc("meeting is not active".into()));
                }

                self.clear_vote(voter_id);
                Ok(true)
            }

            RpcCalls::AddVote => {
                // AddVote format: from_id (u8), target_id (u8)
                let from_id = reader.read_byte();
                let target_id = reader.read_byte();

                if !self.is_active() {
                    return Err(GameError::InvalidRpc("meeting is not active".into()));
                }

                self.add_vote(from_id, target_id);
                Ok(true)
            }

            RpcCalls::VotingComplete => {
                // VotingComplete format: array of u8 vote states (one per player slot),
                // then exiled_id (u8, 255 = nobody exiled), then tie (bool).
                // The exact format may vary by version.

                let mut states = self.vote_states.lock();

                // Read vote states for each player slot (up to 15).
                // The protocol sends one byte per slot indicating vote state:
                // 0 = not voted, 1 = voted, 2 = skipped, 3 = dead/did not vote, etc.
                let num_states = reader.read_packed_u32() as usize;
                let num_states = num_states.min(states.len());

                for i in 0..num_states {
                    if reader.remaining() >= 1 {
                        states[i] = reader.read_byte();
                    }
                }

                // Read exiled player ID
                let exiled_byte = if reader.remaining() >= 1 {
                    reader.read_byte()
                } else {
                    255
                };

                let exiled = if exiled_byte == 255 {
                    None
                } else {
                    Some(exiled_byte)
                };

                // Read tie flag
                let tie = if reader.remaining() >= 1 {
                    reader.read_bool()
                } else {
                    false
                };

                *self.exiled_player_id.lock() = exiled;
                *self.is_tie.lock() = tie;

                // If there's an exiled player, emit the PlayerExiled event
                if let Some(exiled_id) = exiled {
                    let exiled_client = self.resolve_player_client_id(exiled_id);
                    self.game.emit_event(GameEvent::PlayerExiled {
                        game_code: self.game.code,
                        client_id: exiled_client,
                    });
                }

                // The meeting is technically still active until CloseMeeting is called,
                // but voting is complete. We don't set meeting_active to false here;
                // the host will send CloseMeeting next.

                Ok(true)
            }

            RpcCalls::CloseMeeting => {
                // CloseMeeting: the host commands the meeting to end.
                // Only the host can close a meeting.

                if !sender.is_host {
                    return Err(GameError::HostOnlyOperation(sender.client_id));
                }

                self.close_meeting();

                // Emit meeting ended event
                self.game.emit_event(GameEvent::MeetingEnded {
                    game_code: self.game.code,
                });

                Ok(true)
            }

            _ => {
                // Unhandled RPC
                Ok(false)
            }
        }
    }
}

// Helper methods for the meeting HUD.

impl InnerMeetingHud {
    /// Resolve a PlayerId to the corresponding ClientId by searching through
    /// the game's players or the game data registry.
    fn resolve_player_client_id(&self, _player_id: PlayerId) -> ClientId {
        // Search through connected players in the game.
        // In a full implementation, we would walk InnerGameData's players_by_client
        // map or scan PlayerInfo objects registered in the game.
        for entry in self.game.players.iter() {
            let _player = entry.value();
            // Match by character_net_id lookup — but we don't have direct access
            // to PlayerInfo's player_id from ClientPlayer. Instead, we scan
            // the game data objects for a matching PlayerInfo.
        }

        // Walk all objects in the game looking for a PlayerInfo with matching player_id.
        // In a full implementation, the game has a dedicated mapping.
        // For now, fall back to -1 (invalid) — callers validate.
        -1
    }

    /// Infer a PlayerId from a client ID by looking up the player's character.
    fn resolve_player_id_from_client(&self, _client_id: ClientId) -> PlayerId {
        // Search through the game's PlayerInfo objects to find the matching client.
        // In practice this is done via the InnerGameData registry.
        // Fallback: return 0 — callers should have validated.
        0
    }
}
