//! Message router: dispatches incoming Hazel messages to the correct handlers.
//!
//! This is the central dispatch point mimicking the C# `Client.HandleMessageAsync`.
//! Each top-level `MessageFlags` value is routed to the appropriate handler,
//! which calls into the game logic layer and sends responses.

use std::sync::Arc;

use railway_hazel::connection::Connection;
use railway_hazel::message::MessageReader;
use railway_hazel::MessageWriter;
use railway_hazel::SendOption;
use railway_protocol::messages::c2s;
use railway_protocol::messages::s2c;
use railway_protocol::{
    DisconnectReason, GameCode, GameDataTag, MessageFlags, PlatformSpecificData, RpcCalls,
};
use tracing::{debug, info, trace, warn};

use crate::client::Client;
use crate::client_manager::ClientManager;
use crate::game_manager::GameManager;

// ---------------------------------------------------------------------------
// OutgoingMessage
// ---------------------------------------------------------------------------

/// An outgoing message to send to connections or disconnect a client.
pub enum OutgoingMessage {
    /// Send a message writer to a single connection.
    Send {
        connection: Arc<Connection>,
        writer: MessageWriter,
    },
    /// Disconnect a client with a reason.
    Disconnect {
        connection: Arc<Connection>,
        reason: DisconnectReason,
        message: String,
    },
    /// Broadcast a message writer to multiple connections.
    Broadcast {
        connections: Vec<Arc<Connection>>,
        writer: MessageWriter,
    },
}

impl OutgoingMessage {
    pub fn send(connection: Arc<Connection>, writer: MessageWriter) -> Self {
        Self::Send { connection, writer }
    }

    pub fn disconnect(
        connection: Arc<Connection>,
        reason: DisconnectReason,
        message: &str,
    ) -> Self {
        Self::Disconnect {
            connection,
            reason,
            message: message.to_string(),
        }
    }

    pub fn broadcast(connections: Vec<Arc<Connection>>, writer: MessageWriter) -> Self {
        Self::Broadcast {
            connections,
            writer,
        }
    }
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

/// Routes an incoming message from a client.
///
/// Returns a list of messages that should be sent as responses.
pub async fn route_message(
    client: &Arc<Client>,
    reader: &mut MessageReader,
    client_manager: &ClientManager,
    game_manager: &GameManager,
) -> Vec<OutgoingMessage> {
    let tag = reader.tag;

    let flag = match MessageFlags::from_byte(tag) {
        Some(f) => f,
        None => {
            warn!("client {} sent unknown flag: 0x{:02X}", client.id, tag);
            return vec![];
        }
    };

    debug!("client {} ({}) routing flag={:?} (0x{:02X})", client.id, client.name(), flag, tag);

    match flag {
        MessageFlags::HostGame => {
            handle_host_game(client, reader, game_manager).await
        }
        MessageFlags::JoinGame => {
            handle_join_game(client, reader, game_manager, client_manager).await
        }
        MessageFlags::StartGame => {
            handle_start_game(client, reader, game_manager, client_manager).await
        }
        MessageFlags::RemovePlayer => {
            handle_remove_player(client, reader, game_manager, client_manager).await
        }
        MessageFlags::GameData | MessageFlags::GameDataTo => {
            handle_game_data(client, reader, flag, game_manager, client_manager).await
        }
        MessageFlags::EndGame => {
            handle_end_game(client, reader, game_manager, client_manager).await
        }
        MessageFlags::AlterGame => {
            handle_alter_game(client, reader, game_manager, client_manager).await
        }
        MessageFlags::KickPlayer => {
            handle_kick_player(client, reader, game_manager, client_manager).await
        }
        MessageFlags::QueryPlatformIds => {
            handle_query_platform_ids(client, reader, game_manager, client_manager).await
        }
        MessageFlags::GetGameListV2 => {
            // UDP matchmaking is unsupported — tell client to use HTTP
            vec![OutgoingMessage::disconnect(
                client.connection.clone(),
                DisconnectReason::Custom,
                "UDP matchmaking is not supported. Use the HTTP API instead.",
            )]
        }
        MessageFlags::SetActivePodType => {
            handle_set_active_pod_type(client, reader).await
        }
        MessageFlags::PackedGameDataTo => {
            handle_packed_game_data(client, reader, game_manager, client_manager).await
        }
        // Unhandled flags — log and ignore
        MessageFlags::RemoveGame
        | MessageFlags::JoinedGame
        | MessageFlags::WaitForHost
        | MessageFlags::Redirect
        | MessageFlags::ReselectServer
        | MessageFlags::ReportPlayer
        | MessageFlags::QuickMatch
        | MessageFlags::QuickMatchHost
        | MessageFlags::SetGameSession
        | MessageFlags::QueryLobbyInfo
        | MessageFlags::EndGameHostMigration => {
            debug!(
                "client {} sent unhandled flag: {:?}",
                client.id, flag
            );
            vec![]
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// HostGame: create a new game and return the game code.
async fn handle_host_game(
    client: &Arc<Client>,
    reader: &mut MessageReader,
    game_manager: &GameManager,
) -> Vec<OutgoingMessage> {
    let (game_options, _crossplay, filter_options) = c2s::host_game::deserialize(reader);

    // If already in a game, remove first
    if let Some(old_code) = client.game_code() {
        if let Some(old_game) = game_manager.find(old_code) {
            let _ = old_game.remove_player(
                client.id,
                DisconnectReason::ExitGame,
            );
            if old_game.player_count() == 0 {
                game_manager.remove(old_code);
            }
        }
    }

    // Create the game
    let game = match game_manager.create(
        Arc::from(game_options),
        filter_options,
        client.id,
    ) {
        Some(g) => g,
        None => {
            return vec![OutgoingMessage::disconnect(
                client.connection.clone(),
                DisconnectReason::Custom,
                "Failed to create game — all lobby codes are in use",
            )];
        }
    };

    let code = game.code;

    // C# Impostor does NOT add the player during HostGame.
    // The player joins via a separate JoinGame message, and PlayerAdd
    // inside HandleJoinGameNew sets HostId for the first player.
    client.set_game_code(Some(code));

    // Send the game code back to the host
    let mut writer = MessageWriter::new(SendOption::Reliable);
    s2c::host_game::serialize(&mut writer, code);

    info!(
        "HOSTGAME: code={} client={} ({}) mode={:?}",
        code, client.id, client.name(), game.options.game_mode()
    );

    let msgs = vec![OutgoingMessage::send(client.connection.clone(), writer)];
    msgs
}

/// JoinGame: add a player to an existing game.
async fn handle_join_game(
    client: &Arc<Client>,
    reader: &mut MessageReader,
    game_manager: &GameManager,
    client_manager: &ClientManager,
) -> Vec<OutgoingMessage> {
    let game_code = c2s::join_game::deserialize(reader);

    debug!(
        "client {} ({}) joining game {}",
        client.id,
        client.name(),
        game_code
    );

    // Find the game
    let game = match game_manager.find(game_code) {
        Some(g) => g,
        None => {
            warn!("client {} tried to join nonexistent game {}", client.id, game_code);
            return vec![OutgoingMessage::disconnect(
                client.connection.clone(),
                DisconnectReason::GameNotFound,
                "Game not found",
            )];
        }
    };

    // Check game state
    let state = game.state();
    if !state.can_join() {
        let reason = match state {
            railway_game_logic::GameState::Started => DisconnectReason::GameStarted,
            railway_game_logic::GameState::Ended => DisconnectReason::GameNotFound,
            railway_game_logic::GameState::Destroyed => DisconnectReason::GameNotFound,
            _ => DisconnectReason::GameStarted,
        };
        return vec![OutgoingMessage::disconnect(
            client.connection.clone(),
            reason,
            "Cannot join game in current state",
        )];
    }

    // Check for ban
    let client_ip = client.connection.endpoint().ip();
    if game.is_ip_banned(client_ip) {
        warn!(
            "banned client {} ({}) tried to join game {}",
            client.id,
            client.name(),
            game_code
        );
        return vec![OutgoingMessage::disconnect(
            client.connection.clone(),
            DisconnectReason::Banned,
            "You are banned from this game",
        )];
    }

    // Remove from current game if any
    if let Some(old_code) = client.game_code() {
        if old_code != game_code {
            if let Some(old_game) = game_manager.find(old_code) {
                let _ = old_game.remove_player(client.id, DisconnectReason::ExitGame);
                if old_game.player_count() == 0 {
                    game_manager.remove(old_code);
                }
            }
        }
    }

    // Add the player to the game
    match game.add_player(client.id, client.name.clone()) {
        Ok(()) => {
            client.set_game_code(Some(game_code));
        }
        Err(e) => {
            warn!(
                "client {} failed to join game {}: {}",
                client.id, game_code, e
            );
            return vec![OutgoingMessage::disconnect(
                client.connection.clone(),
                DisconnectReason::GameFull,
                &e.to_string(),
            )];
        }
    }

    // Build the JoinedGame response with all OTHER players (must exclude
    // the joining client itself — see C#'s `WriteJoinedGameMessage`:
    // `_players.Where(x => x.Value != player)`). The joining client is
    // identified separately via the `client_id` field in the message;
    // including it again in this list produces a self-referential entry
    // that real clients don't expect and appears to cause them to drop
    // the whole list (showing 0 players instead of the actual count).
    let host_id = game.host_id();
    info!(
        "JOIN: client_id={} host_id={} game_code={} player_count={}",
        client.id, host_id, game_code, game.player_count()
    );
    let players: Vec<s2c::joined_game::JoinedPlayerInfo> = game
        .players
        .iter()
        .filter(|p| p.client_id != client.id)
        .map(|p| {
            let platform_data = client_manager
                .get(p.client_id)
                .and_then(|c| c.platform_data.clone())
                .unwrap_or_else(|| PlatformSpecificData {
                    platform: railway_protocol::platform_data::Platform::Unknown,
                    platform_name: String::new(),
                    xbox_platform_id: None,
                    psn_platform_id: None,
                });
            s2c::joined_game::JoinedPlayerInfo {
                client_id: p.client_id,
                name: p.name.clone(),
                platform_data,
                player_level: 1,
            }
        })
        .collect();

    // Send JoinedGame + AlterGame to the joining client (matching C#
    // HandleJoinGameNew which writes both into the same MessageWriter
    // before sending).
    let mut writer = MessageWriter::new(SendOption::Reliable);
    s2c::joined_game::serialize_join(
        &mut writer,
        true,
        game_code,
        client.id,
        host_id,
        &players,
    );
    // Also send the current public/private state so the joining client
    // shows the correct toggle state in the lobby UI.
    s2c::alter_game::serialize(
        &mut writer,
        false,
        game_code,
        game.is_public.load(std::sync::atomic::Ordering::SeqCst),
    );

    info!(
        "JOIN_OK: client={} name={} code={} player_count={} is_public={}",
        client.id, client.name(), game_code, game.player_count(),
        game.is_public.load(std::sync::atomic::Ordering::SeqCst)
    );

    let mut outgoing = vec![OutgoingMessage::send(client.connection.clone(), writer)];

    // NOTE: we deliberately do NOT synthesize a fake "PlayerInfo spawn"
    // message here. Per the real C# reference (`Game.Data.cs`), the
    // server NEVER spawns InnerNetObjects on a client's behalf -- spawning
    // is always initiated by the HOST client itself (it sends a real
    // SpawnFlag GameData message once it's ready), and the server's job
    // is only to register the resulting NetIds and relay them to other
    // players. A hand-built spawn payload here doesn't match the real
    // wire format for PlayerControl/GameData components and is more
    // likely to confuse a real client than help it.

    // Broadcast JoinGame to everyone else already in the lobby so their
    // player list/count stays in sync with the new arrival. Without this,
    // only the joining client learns about itself — everyone else's UI
    // goes stale (this was the "player count/settings wrong" bug).
    let other_connections: Vec<Arc<Connection>> = game
        .players
        .iter()
        .filter(|p| p.client_id != client.id)
        .filter_map(|p| client_manager.get(p.client_id).map(|c| c.connection.clone()))
        .collect();

    if !other_connections.is_empty() {
        let client_platform_data = client
            .platform_data
            .clone()
            .unwrap_or_else(|| PlatformSpecificData {
                platform: railway_protocol::platform_data::Platform::Unknown,
                platform_name: String::new(),
                xbox_platform_id: None,
                psn_platform_id: None,
            });

        let mut join_writer = MessageWriter::new(SendOption::Reliable);
        s2c::join_game::serialize_join(
            &mut join_writer,
            true,
            game_code,
            client.id,
            host_id,
            client.name(),
            &client_platform_data,
            1,
        );

        outgoing.push(OutgoingMessage::Broadcast {
            connections: other_connections,
            writer: join_writer,
        });
    }

    outgoing
}

/// StartGame: transition the game to Starting/Started and relay to all players.
async fn handle_start_game(
    client: &Arc<Client>,
    reader: &mut MessageReader,
    game_manager: &GameManager,
    client_manager: &ClientManager,
) -> Vec<OutgoingMessage> {
    // Client always sends the game code as the first i32 in the StartGame
    // message body. Consume it so the reader position is correct.
    let _msg_game_code = reader.read_i32();

    let game_code = match client.game_code() {
        Some(c) => c,
        None => {
            warn!(
                "client {} ({}) sent StartGame but is not in a game",
                client.id,
                client.name()
            );
            return vec![];
        }
    };

    let game = match game_manager.find(game_code) {
        Some(g) => g,
        None => {
            warn!("client {} start game: game {} not found", client.id, game_code);
            return vec![];
        }
    };

    // Verify the client is the host
    if game.host_id() != client.id {
        warn!(
            "client {} ({}) tried to start game {} but is not the host (host is {})",
            client.id,
            client.name(),
            game_code,
            game.host_id()
        );
        return vec![];
    }

    debug!(
        "client {} ({}) starting game {}",
        client.id,
        client.name(),
        game_code
    );

    // Transition the game state
    let _ = railway_game_logic::game_flow::start_game(&game).await;

    // Relay the StartGame message to all players in the game, VERBATIM —
    // matches real C#'s `HandleStartGame`: `message.CopyTo(packet); await
    // SendToAllAsync(packet);`. A previous version reconstructed a bare
    // message containing only `game_code`, silently dropping any other
    // fields the real client might send in this message.
    reader.seek(0);
    let mut writer = MessageWriter::new(SendOption::Reliable);
    writer.start_message(MessageFlags::StartGame as u8);
    writer.copy_from_reader(reader);
    writer.end_message();

    let connections = get_game_connections(&game, client_manager);

    info!("game {} started by host {}", game_code, client.name());

    vec![OutgoingMessage::broadcast(connections, writer)]
}

/// RemovePlayer: remove a player from the game.
async fn handle_remove_player(
    client: &Arc<Client>,
    reader: &mut MessageReader,
    game_manager: &GameManager,
    client_manager: &ClientManager,
) -> Vec<OutgoingMessage> {
    let (player_id, reason_byte) = c2s::remove_player::deserialize(reader);

    debug!(
        "client {} removing player {} (reason={})",
        client.id, player_id, reason_byte
    );

    let game_code = match client.game_code() {
        Some(c) => c,
        None => return vec![],
    };

    let game = match game_manager.find(game_code) {
        Some(g) => g,
        None => return vec![],
    };

    // Verify the requester is the host
    if game.host_id() != client.id {
        warn!(
            "client {} tried to remove player but is not the host",
            client.id
        );
        return vec![];
    }

    let reason = DisconnectReason::from_byte(reason_byte);

    // Remove the player from the game
    let _ = game.remove_player(player_id as i32, reason);

    // If the removed client is still connected, disconnect them
    let mut outgoing = vec![];
    if let Some(target_client) = client_manager.get(player_id as i32) {
        target_client.set_game_code(None);
        outgoing.push(OutgoingMessage::disconnect(
            target_client.connection.clone(),
            reason,
            reason.description(),
        ));
    }

    // Relay to remaining players
    let host_id = game.host_id();
    let mut writer = MessageWriter::new(SendOption::Reliable);
    s2c::remove_player::serialize(
        &mut writer,
        true,
        game_code,
        player_id as i32,
        host_id,
        reason,
    );
    let connections = get_game_connections(&game, client_manager);
    outgoing.push(OutgoingMessage::broadcast(connections, writer));

    // Clean up empty games
    if game.player_count() == 0 {
        game_manager.remove(game_code);
    }

    outgoing
}

/// GameData / GameDataTo: dispatch inner GameDataTag sub-messages to
/// InnerNetObjects for deserialization or RPC handling.
async fn handle_game_data(
    client: &Arc<Client>,
    reader: &mut MessageReader,
    flag: MessageFlags,
    game_manager: &GameManager,
    client_manager: &ClientManager,
) -> Vec<OutgoingMessage> {
    let game_code = match client.game_code() {
        Some(c) => c,
        None => {
            warn!(
                "client {} sent GameData but is not in a game",
                client.id
            );
            return vec![];
        }
    };

    let game = match game_manager.find(game_code) {
        Some(g) => g,
        None => {
            warn!(
                "client {} sent GameData for nonexistent game {}",
                client.id, game_code
            );
            return vec![];
        }
    };

    // Client always sends the game code as the first fixed i32 in the
    // GameData / GameDataTo body. Skip past it so the sub-message loop
    // starts at the real payload.
    let _msg_game_code = reader.read_i32();

    // IMPORTANT: `GameDataTo` has a DIFFERENT wire layout than `GameData`
    // — it carries an extra `target client id` (packed i32) immediately
    // after gameCode, BEFORE the actual GameDataTag entries. Matches
    // real C#'s `Client.HandleMessageAsync`:
    //   var toPlayer = flag == MessageFlags.GameDataTo;
    //   ... var target = reader.ReadPackedInt32(); (read before relaying)
    // A previous version always jumped straight into the GameDataTag
    // loop regardless of flag, so for GameDataTo the target-id bytes got
    // misread as a bogus sub-message length, breaking the parse
    // immediately (0 spawns/data/rpcs parsed even for large payloads).
    let target_client_id: Option<i32> = if flag == MessageFlags::GameDataTo {
        Some(reader.read_packed_i32())
    } else {
        None
    };

    // Track the current position so we can read sub-messages
    let start_pos = reader.position();
    let mut outgoing = vec![];

    // Process each GameDataTag sub-message
    let mut spawn_count = 0u32;
    let mut data_count = 0u32;
    let mut rpc_count = 0u32;
    // Matches real C#'s `HandleGameDataAsync` return value: the WHOLE
    // message is validated as one unit — if ANY entry inside it fails an
    // anti-cheat check, the entire raw-copy relay is skipped (not just
    // that one entry), matching `Client.cs`'s
    // `if (verified) { /* relay */ }` gate.
    let mut verified = true;
    while let Some(mut sub_reader) = reader.read_message() {
        let tag = match GameDataTag::from_byte(sub_reader.tag) {
            Some(t) => t,
            None => {
                warn!(
                    "client {} sent unknown GameDataTag: 0x{:02X}",
                    client.id, sub_reader.tag
                );
                continue;
            }
        };

        match tag {
            GameDataTag::DataFlag => {
                data_count += 1;
                let net_id = sub_reader.read_packed_u32();

                // Ownership check, same as RpcFlag below — e.g. real C#'s
                // `InnerVoteBanSystem.DeserializeAsync` calls
                // `ValidateHost(...)`. IMPORTANT SCOPE LIMIT: we only
                // enforce this when the object's owner is a genuine,
                // specific player (owner_id > 0) — e.g. "player A sending
                // data for player B's PlayerControl" is unambiguously
                // wrong. For host-inherited globals (owner_id == -2, e.g.
                // VoteBanSystem/NormalGameManager/LobbyBehaviour/
                // MeetingHud), real C# validates each RPC/Data type with
                // ITS OWN specific rule (`ValidateHost` for some,
                // `ValidateCmd`/`ValidateBroadcast` for others like
                // CastVote, which non-host players ARE allowed to send).
                // A single generic "owner-or-host" rule applied to those
                // objects would incorrectly reject legitimate actions
                // like ordinary players casting votes, so we deliberately
                // skip enforcement there until each RPC gets its own
                // correctly-scoped check.
                if client_manager.anticheat.enabled && client_manager.anticheat.enable_ownership_checks {
                if let Some(owner_id) = game.net_id_owners.get(&net_id).map(|r| *r.value()) {
                    if owner_id > 0 {
                        let is_host = game.host_id() == client.id;
                        let result = railway_game_logic::anticheat_checks::check_ownership(
                            owner_id, client.id, is_host,
                        );
                        if result.is_cheat() {
                            warn!(
                                "client {} DataFlag rejected (ownership): {}",
                                client.id,
                                result.cheat_message().unwrap_or("")
                            );
                            verified = false;
                            continue;
                        }
                    }
                }
                }

                if let Some(_obj) = game.find_object(net_id) {
                    trace!("client {} DataFlag netId={}", client.id, net_id);
                }
            }

            GameDataTag::RpcFlag => {
                rpc_count += 1;
                let net_id = sub_reader.read_packed_u32();
                let rpc_byte = sub_reader.read_byte();
                let rpc = RpcCalls::from_byte(rpc_byte);

                // Ownership check — matches real C#'s
                // `InnerNetObject.IsOwnedBy`: a player may only send RPCs
                // for objects they own, UNLESS they're the host. This was
                // completely unwired before: any client could send e.g.
                // SetColor/SetName/MurderPlayer RPCs targeting an object
                // owned by a DIFFERENT player.
                //
                // SCOPE LIMIT (see the matching note in the DataFlag arm
                // above): only enforced when owner_id is a genuine,
                // specific player (> 0). Host-inherited globals (owner_id
                // == -2, e.g. MeetingHud) each have their OWN specific
                // validation rule in real C# — for example CastVote is
                // explicitly allowed from non-host senders via
                // `ValidateCmd`, so a blanket "owner-or-host" rule here
                // would wrongly block ordinary players from voting.
                if client_manager.anticheat.enabled && client_manager.anticheat.enable_ownership_checks {
                if let Some(owner_id) = game.net_id_owners.get(&net_id).map(|r| *r.value()) {
                    if owner_id > 0 {
                        let is_host = game.host_id() == client.id;
                        let result = railway_game_logic::anticheat_checks::check_ownership(
                            owner_id, client.id, is_host,
                        );
                        if result.is_cheat() {
                            warn!(
                                "client {} RPC rejected (ownership): {}",
                                client.id,
                                result.cheat_message().unwrap_or("")
                            );
                            verified = false;
                            continue;
                        }
                    }
                }
                }

                if let Some(rpc_call) = rpc {
                    // Field layouts verified against the real Rpc*.cs
                    // classes in Impostor.Api.Net.Messages.Rpcs. Two
                    // easy-to-miss traps confirmed from that source:
                    // - SetName/SetColor's payload has its OWN leading
                    //   `netId: u32` (FIXED, not packed) field before the
                    //   actual value — separate from the outer netId
                    //   already read above. Skipping it misreads the
                    //   name/color entirely.
                    // - CheckColor's value is a single byte
                    //   (`(ColorType)reader.ReadByte()`), NOT a packed
                    //   i32.
                    match rpc_call {
                        RpcCalls::SendChat => {
                            // Rpc13SendChat: message:string
                            let text = sub_reader.read_string();
                            info!(
                                "{}[ClientId:{}, Ip:{}] send chat: {}",
                                client.name(),
                                client.id,
                                client.connection.endpoint().ip(),
                                text
                            );
                        }
                        RpcCalls::SendChatNote => {
                            let note_type = sub_reader.read_byte();
                            debug!("{} CHAT_NOTE type={}", client.name(), note_type);
                        }
                        RpcCalls::CheckName => {
                            let name = sub_reader.read_string();
                            debug!("{} CheckName: '{}'", client.name(), name);
                        }
                        RpcCalls::SetName => {
                            let _inner_net_id = sub_reader.read_u32();
                            let name = sub_reader.read_string();
                            info!("{} set name: '{}'", client.name(), name);
                        }
                        RpcCalls::CheckColor => {
                            let color = sub_reader.read_byte();
                            debug!("{} CheckColor: {}", client.name(), color);
                        }
                        RpcCalls::SetColor => {
                            let _inner_net_id = sub_reader.read_u32();
                            let color = sub_reader.read_byte();
                            info!("{} set color: {}", client.name(), color);
                        }
                        RpcCalls::SetHatStr => {
                            let hat = sub_reader.read_string();
                            let _seq = sub_reader.read_byte();
                            debug!("{} set hat: {}", client.name(), hat);
                        }
                        RpcCalls::SetSkinStr => {
                            let skin = sub_reader.read_string();
                            let _seq = sub_reader.read_byte();
                            debug!("{} set skin: {}", client.name(), skin);
                        }
                        RpcCalls::SetPetStr => {
                            let pet = sub_reader.read_string();
                            debug!("{} set pet: {}", client.name(), pet);
                        }
                        RpcCalls::SetVisorStr => {
                            let visor = sub_reader.read_string();
                            let _seq = sub_reader.read_byte();
                            debug!("{} set visor: {}", client.name(), visor);
                        }
                        RpcCalls::SetNamePlateStr => {
                            let plate = sub_reader.read_string();
                            let _seq = sub_reader.read_byte();
                            debug!("{} set nameplate: {}", client.name(), plate);
                        }
                        RpcCalls::SetLevel => {
                            let level = sub_reader.read_packed_u32();
                            debug!("{} set level: {}", client.name(), level);
                        }
                        RpcCalls::MurderPlayer => {
                            let target_net_id = sub_reader.read_packed_u32();
                            let result = sub_reader.read_i32();
                            info!("{} murdered netId={} result={}", client.name(), target_net_id, result);
                        }
                        RpcCalls::StartMeeting => {
                            let target_net_id = sub_reader.read_byte();
                            info!("{} started meeting target={}", client.name(), target_net_id);
                        }
                        RpcCalls::CompleteTask => {
                            let task_idx = sub_reader.read_packed_u32();
                            debug!(
                                "client {} COMPLETE_TASK netId={} taskIdx={}",
                                client.id, net_id, task_idx
                            );
                        }
                        _ => {
                            debug!("client {} RPC {:?} netId={}", client.id, rpc_call, net_id);
                        }
                    }
                } else {
                    debug!(
                        "client {} unknown RPC byte=0x{:02X} netId={}",
                        client.id, rpc_byte, net_id
                    );
                }
            }

            GameDataTag::SpawnFlag => {
                spawn_count += 1;

                // Matches C#'s Game.Data.cs SpawnFlag handling exactly:
                //   objectId: packed_u32
                //   ownerClientId: packed_i32
                //   spawnFlags: byte
                //   componentsCount: packed_i32
                //   for each component: netId (packed_u32) + sub-message (initial state)
                // A previous version of this handler stopped after reading
                // just the first 3 fields and never consumed the component
                // list at all, so the server never registered any object —
                // every later DataFlag/RpcFlag lookup by NetId silently
                // failed to find anything.
                let spawn_type = sub_reader.read_packed_u32();
                let owner_id = sub_reader.read_packed_i32();
                let spawn_flags_byte = sub_reader.read_byte();
                let component_count = sub_reader.read_packed_i32();
                let name = railway_game_logic::objects::spawn_registry::spawnable_name(spawn_type);

                debug!(
                    "client {} SPAWN type={}({}) owner={} flags=0x{:02X} components={}",
                    client.id, spawn_type, name, owner_id, spawn_flags_byte, component_count
                );

                if component_count < 0 || component_count > 16 {
                    warn!(
                        "client {} sent SpawnFlag with implausible component_count={} — ignoring",
                        client.id, component_count
                    );
                    continue;
                }

                // Create one server-side instance to represent this spawn.
                // NOTE: our object model doesn't yet expose individual
                // child components (PlayerControl/PlayerPhysics/
                // CustomNetworkTransform are conceptually 3 separate NetIds
                // in the real protocol), so for multi-component spawns we
                // register the SAME instance under every NetId reported.
                // That's not fully semantically correct, but it means a
                // DataFlag/RpcFlag on any of those NetIds resolves to a
                // real object instead of silently failing, which is the
                // immediate bug we're fixing.
                let instance = railway_game_logic::objects::spawn_registry::create_spawnable(
                    spawn_type,
                    Arc::clone(&game),
                );
                if instance.is_none() {
                    warn!(
                        "client {} sent SpawnFlag for unregistered type {} ({}) — not tracked",
                        client.id, spawn_type, name
                    );
                }

                let mut first_net_id = None;
                for _ in 0..component_count {
                    let net_id = sub_reader.read_packed_u32();
                    if first_net_id.is_none() {
                        first_net_id = Some(net_id);
                    }
                    // Each component carries its own length-prefixed
                    // sub-message with its initial state. We don't
                    // deserialize component state yet, but we must still
                    // consume it so the reader stays aligned.
                    let _component_data = sub_reader.read_message();

                    if let Some(obj) = &instance {
                        game.register_object(net_id, Arc::clone(obj));
                    }
                    // Track ownership regardless of whether we could
                    // construct a real instance — the ownership check
                    // only needs the owner id, not the object itself.
                    game.net_id_owners.insert(net_id, owner_id);
                }

                // NOTE: previously this cached spawns with owner == -2
                // (VoteBanSystem/NormalGameManager/LobbyBehaviour, which
                // are host-spawned globals) under the assumption that the
                // server needed to replay them to late joiners. Checking
                // the real C# source (`Game.Data.cs`) shows this is wrong:
                // `SyncServerObjectsAsync` only re-syncs objects whose
                // `OwnerId == ServerOwned` (-4), which is the constant
                // used exclusively for the SERVER'S OWN spawns (i.e.
                // `PlayerInfo`, see `SpawnPlayerInfoAsync`). Objects with
                // owner == -2 ("InvalidClient", host-spawned globals) are
                // NOT resynced this way at all — that's the host client's
                // own responsibility via ordinary GameData/GameDataTo
                // traffic, which we now relay correctly. Caching them
                // here was based on a mistaken assumption and risked
                // sending duplicate spawns on top of whatever the host
                // itself sends. See the SceneChangeFlag branch below for
                // where PlayerInfo (owner == -4) gets cached instead.

                // Track the PlayerControl's NetId on the owning player for
                // later features (kill validation, etc.) that need to map
                // "this NetId" back to "this client".
                if name == "PlayerControl" {
                    if let Some(net_id) = first_net_id {
                        if let Some(mut p) = game.players.get_mut(&owner_id) {
                            p.character_net_id = Some(net_id);
                        }
                    }
                }
            }

            GameDataTag::DespawnFlag => {
                let net_id = sub_reader.read_packed_u32();
                debug!("client {} DESPAWN netId={}", client.id, net_id);
                game.unregister_object(net_id);
            }

            GameDataTag::SceneChangeFlag => {
                let target_id = sub_reader.read_packed_i32();
                let scene = sub_reader.read_string();
                info!("SCENE: client={} -> '{}' target={}", client.id, scene, target_id);
                if let Some(mut player) = game.players.get_mut(&target_id) {
                    player.scene = Some(scene.clone());
                }

                // Matches C#'s real SceneChangeFlag handling exactly:
                // once a client announces it has loaded into "OnlineGame",
                // the SERVER (not the host client!) is responsible for
                // spawning that player's PlayerInfo object and
                // broadcasting it to everyone in the game — see
                // `Game.Data.cs`: `await SpawnPlayerInfoAsync(sender);`.
                //
                // This step was completely missing before. The host's own
                // client sits in `AmongUsClient.OnPlayerJoined` waiting for
                // `ClientData.InScene` to become true, which itself only
                // happens once a PlayerInfo/PlayerControl spawn for that
                // client is received (see the game's `CoHandleSpawn`) —
                // so without this, a solo host would wait forever with
                // zero players ever appearing, exactly what we were
                // seeing.
                if scene == "OnlineGame" && target_id == client.id {
                    // SyncServerObjectsAsync equivalent: replay every
                    // PREVIOUSLY-SPAWNED server-owned PlayerInfo (OwnerId
                    // == ServerOwned == -4, per real C#'s
                    // `SpawnPlayerInfoAsync`/`SyncServerObjectsAsync`) to
                    // JUST this newly-scened client, before spawning its
                    // own PlayerInfo. Without this, a second (or later)
                    // player joining an already-set-up lobby never learns
                    // the earlier players' PlayerInfo exists at all.
                    if !game.server_owned_spawn_cache.is_empty() {
                        for entry in game.server_owned_spawn_cache.iter() {
                            let gd = wrap_spawn_flag_message(game_code, entry.value());
                            outgoing.push(OutgoingMessage::send(client.connection.clone(), gd));
                        }
                        info!(
                            "SYNC: replayed {} server-owned PlayerInfo object(s) to client={} on scene entry",
                            game.server_owned_spawn_cache.len(),
                            target_id
                        );
                    }

                    let already_spawned = game
                        .players
                        .get(&target_id)
                        .map(|p| p.player_info_net_id.is_some())
                        .unwrap_or(true);

                    if !already_spawned {
                        // IMPORTANT: must be the first FREE slot (matches
                        // real C#'s `GetNextAvailablePlayerId`), not "how
                        // many players currently have a PlayerInfo". The
                        // count-based approach collides the moment a
                        // player leaves and a new one joins afterward:
                        // e.g. players get slots 0,1,2; player at slot 1
                        // leaves; a count of remaining players (2) would
                        // hand the new joiner slot 2 — already taken by
                        // the third player.
                        let player_id_slot = game.next_available_player_id();
                        let net_id = game.next_net_id();

                        let payload = build_player_info_spawn_payload(net_id, target_id, player_id_slot);
                        let spawn_writer = wrap_spawn_flag_message(game_code, &payload);

                        // Cache this PlayerInfo's raw payload too, so the
                        // NEXT player who joins gets it replayed via the
                        // sync step above.
                        game.server_owned_spawn_cache.insert(net_id, payload);

                        if let Some(mut p) = game.players.get_mut(&target_id) {
                            p.player_info_net_id = Some(net_id);
                            p.player_id_slot = Some(player_id_slot);
                        }

                        info!(
                            "PLAYERINFO: spawning for client={} netId={} playerId={}",
                            target_id, net_id, player_id_slot
                        );

                        let all_connections = get_game_connections(&game, client_manager);
                        outgoing.push(OutgoingMessage::broadcast(all_connections, spawn_writer));
                    }
                }
            }

            GameDataTag::ReadyFlag => {
                let ready_client_id = sub_reader.read_packed_i32();
                debug!("client {} READY target={}", client.id, ready_client_id);
            }

            GameDataTag::ChangeSettingsFlag => {
                info!("SETTINGS: client {} changed game settings", client.id);
            }

            GameDataTag::ConsoleDeclareClientPlatformFlag => {
                let platform_str = sub_reader.read_string();
                debug!("client {} PLATFORM={}", client.id, platform_str);
            }

            GameDataTag::PS4RoomRequestFlag => {
                debug!("client {} PS4 room request (ignored)", client.id);
            }
        }
    }

if spawn_count > 0 || rpc_count > 0 {
                info!(
                    "GAMEDATA: spawns={} rpcs={}",
                    spawn_count, rpc_count
                );
            } else {
                debug!(
                    "GAMEDATA: spawns={} rpcs={} data={}",
                    spawn_count, rpc_count, data_count
                );
            }

    // Relay — standard GameData broadcast to other players only:
    //   • Relay to OTHER players (standard GameData broadcast)
    //   • Echo SpawnFlag sub-messages back to SENDER.
    //     C#'s OnSpawnAsync → SendObjectSpawnAsync does this so the
    //     sender's CreatePlayer can see PlayerInfo via HasPlayer().
    if !verified {
        warn!(
            "client {} sent a GameData/GameDataTo message with at least one \
             failed anti-cheat check — dropping the relay entirely (matches \
             real C#'s all-or-nothing `HandleGameDataAsync` gate, not a \
             per-entry filter)",
            client.id
        );
        return outgoing;
    }

    if flag == MessageFlags::GameData {
        reader.seek(start_pos);

        // ---- relay to other players (excludes sender) ----
        {
            let mut writer = MessageWriter::new(SendOption::Reliable);
            writer.start_message(MessageFlags::GameData as u8);
            writer.write_i32(game_code);
            writer.copy_from_reader(reader);
            writer.end_message();

            let others: Vec<Arc<Connection>> = get_game_connections(&game, client_manager)
                .into_iter()
                .filter(|c| c.endpoint() != client.connection.endpoint())
                .collect();
            if !others.is_empty() {
                outgoing.push(OutgoingMessage::broadcast(others, writer));
            }
        }

        // ---- echo PlayerInfo/PlayerControl spawns back to sender ----
        // C#'s OnSpawnAsync only calls SendObjectSpawnAsync for PlayerInfo,
        // NOT for every spawn. Echoing all spawns causes the client to
        // destroy its own objects as duplicates ("AddNetObject → false →
        // destroy parent"). We only echo spawns that create game-data
        // entries the sender is waiting for in CreatePlayer::HasPlayer().
        // PlayerInfo echo removed — echoing all spawns causes
        // duplicate object destruction on the client. C# Impostor
        // only calls SendObjectSpawnAsync for server-created
        // PlayerInfo, not for every spawn. PlayerInfo is now
        // sent proactively in handle_join_game instead.
    } else if flag == MessageFlags::GameDataTo {
        // Matches real C#'s targeted relay exactly:
        //   var target = reader.ReadPackedInt32();
        //   reader.CopyTo(writer);
        //   await Player.Game.SendToAsync(writer, target);
        // A previous version never relayed GameDataTo at all — the
        // target client id bytes were even being misread as part of the
        // GameDataTag loop (fixed above), so this message type was
        // completely non-functional end-to-end.
        reader.seek(start_pos);

        if let Some(target_id) = target_client_id {
            if let Some(target_client) = client_manager.get(target_id) {
                let mut writer = MessageWriter::new(SendOption::Reliable);
                writer.start_message(MessageFlags::GameData as u8);
                writer.write_i32(game_code);
                writer.copy_from_reader(reader);
                writer.end_message();

                outgoing.push(OutgoingMessage::send(target_client.connection.clone(), writer));
            } else {
                warn!(
                    "client {} sent GameDataTo target={} but that client doesn't exist",
                    client.id, target_id
                );
            }
        }
    }

    outgoing
}

/// EndGame: the host ends the game.
async fn handle_end_game(
    client: &Arc<Client>,
    reader: &mut MessageReader,
    game_manager: &GameManager,
    client_manager: &ClientManager,
) -> Vec<OutgoingMessage> {
    let reason = c2s::end_game::deserialize(reader);

    let game_code = match client.game_code() {
        Some(c) => c,
        None => return vec![],
    };

    let game = match game_manager.find(game_code) {
        Some(g) => g,
        None => return vec![],
    };

    // Verify the client is the host
    if game.host_id() != client.id {
        warn!(
            "client {} tried to end game {} but is not the host",
            client.id, game_code
        );
        return vec![];
    }

    debug!(
        "client {} ({}) ending game {}: reason={:?}",
        client.id,
        client.name(),
        game_code,
        reason
    );

    // End the game
    railway_game_logic::game_flow::end_game(&game, reason).await;

    // Relay to all players, VERBATIM — matches real C#'s `HandleEndGame`:
    // `message.CopyTo(packet); await SendToAllAsync(packet);`. A previous
    // version reconstructed a bare `[game_code][reason]` message, which
    // silently dropped the trailing `showAd: bool` field the real client
    // sends (see `Message08EndGameC2S.Deserialize`), and — more
    // importantly — would keep dropping any other field a future client
    // version adds here, the same class of bug fixed for StartGame above.
    reader.seek(0);
    let mut writer = MessageWriter::new(SendOption::Reliable);
    writer.start_message(MessageFlags::EndGame as u8);
    writer.copy_from_reader(reader);
    writer.end_message();

    let connections = get_game_connections(&game, client_manager);

    info!(
        "game {} ended by host {} (reason: {:?})",
        game_code, client.name(), reason
    );

    vec![OutgoingMessage::broadcast(connections, writer)]
}

/// AlterGame: change game privacy (public/private).
async fn handle_alter_game(
    client: &Arc<Client>,
    reader: &mut MessageReader,
    game_manager: &GameManager,
    client_manager: &ClientManager,
) -> Vec<OutgoingMessage> {
    let (_tag, is_public) = c2s::alter_game::deserialize(reader);

    let game_code = match client.game_code() {
        Some(c) => c,
        None => return vec![],
    };

    let game = match game_manager.find(game_code) {
        Some(g) => g,
        None => return vec![],
    };

    // Verify the client is the host
    if game.host_id() != client.id {
        warn!(
            "client {} tried to alter game {} but is not the host",
            client.id, game_code
        );
        return vec![];
    }

    info!(
        "ALTER_GAME: client={} is_public={} host_check={}",
        client.id, is_public, game.host_id() == client.id
    );

    game.is_public.store(is_public, std::sync::atomic::Ordering::SeqCst);

    // Relay to all players
    let mut writer = MessageWriter::new(SendOption::Reliable);
    s2c::alter_game::serialize(&mut writer, true, game_code, is_public);

    let connections = get_game_connections(&game, client_manager);

    vec![OutgoingMessage::broadcast(connections, writer)]
}

/// KickPlayer: kick or ban a player from the game.
async fn handle_kick_player(
    client: &Arc<Client>,
    reader: &mut MessageReader,
    game_manager: &GameManager,
    client_manager: &ClientManager,
) -> Vec<OutgoingMessage> {
    let (client_id_to_kick, is_ban) = c2s::kick_player::deserialize(reader);

    let game_code = match client.game_code() {
        Some(c) => c,
        None => return vec![],
    };

    let game = match game_manager.find(game_code) {
        Some(g) => g,
        None => return vec![],
    };

    // Verify the client is the host
    if game.host_id() != client.id {
        warn!(
            "client {} tried to kick player from game {} but is not the host",
            client.id, game_code
        );
        return vec![];
    }

    let target_id = client_id_to_kick;
    let reason = if is_ban {
        DisconnectReason::Banned
    } else {
        DisconnectReason::Kicked
    };

    debug!(
        "client {} kicking player {} from game {} (ban={})",
        client.id, target_id, game_code, is_ban
    );

    // If banning, record the IP
    if is_ban {
        if let Some(target_client) = client_manager.get(target_id) {
            let ip = target_client.connection.endpoint().ip();
            game.ban_ip(ip);
        }
    }

    // Remove the player from the game
    let _ = game.remove_player(target_id, reason);

    let mut outgoing = vec![];

    // Disconnect the target client
    if let Some(target_client) = client_manager.get(target_id) {
        target_client.set_game_code(None);
        outgoing.push(OutgoingMessage::disconnect(
            target_client.connection.clone(),
            reason,
            reason.description(),
        ));
    }

    // Relay to remaining players
    let mut writer = MessageWriter::new(SendOption::Reliable);
    s2c::kick_player::serialize(&mut writer, true, game_code, target_id, is_ban);

    let connections = get_game_connections(&game, client_manager);
    outgoing.push(OutgoingMessage::broadcast(connections, writer));

    if game.player_count() == 0 {
        game_manager.remove(game_code);
    }

    outgoing
}

/// QueryPlatformIds: collect platform data for players in a game and respond.
async fn handle_query_platform_ids(
    client: &Arc<Client>,
    reader: &mut MessageReader,
    game_manager: &GameManager,
    client_manager: &ClientManager,
) -> Vec<OutgoingMessage> {
    let game_code = c2s::query_platform_ids::deserialize(reader);

    debug!(
        "client {} querying platform IDs for game {}",
        client.id, game_code
    );

    let game = match game_manager.find(game_code) {
        Some(g) => g,
        None => return vec![],
    };

    // Collect platform data from all players in the game
    let platform_data: Vec<railway_protocol::platform_data::PlatformSpecificData> = game
        .players
        .iter()
        .filter_map(|p| {
            client_manager
                .get(p.client_id)
                .and_then(|c| c.platform_data.clone())
        })
        .collect();

    let mut writer = MessageWriter::new(SendOption::Reliable);
    s2c::query_platform_ids::serialize(
        &mut writer,
        game_code,
        &platform_data,
    );

    vec![OutgoingMessage::send(client.connection.clone(), writer)]
}

/// SetActivePodType: client declares its platform type.
async fn handle_set_active_pod_type(
    client: &Arc<Client>,
    reader: &mut MessageReader,
) -> Vec<OutgoingMessage> {
    let pod_type = c2s::set_active_pod_type::deserialize(reader);

    debug!(
        "client {} set active pod type: {}",
        client.id, pod_type
    );

    // In production, store this on the client for platform-aware features
    vec![]
}

/// PackedGameDataTo: unpack and process each inner GameDataTo message.
async fn handle_packed_game_data(
    client: &Arc<Client>,
    reader: &mut MessageReader,
    game_manager: &GameManager,
    client_manager: &ClientManager,
) -> Vec<OutgoingMessage> {
    debug!(
        "client {} sent PackedGameDataTo",
        client.id
    );

    let mut outgoing = vec![];

    // Read each inner GameDataTo sub-message
    while let Some(mut inner_reader) = reader.read_message() {
        // The inner message should have tag = GameDataTo (6)
        if inner_reader.tag == MessageFlags::GameDataTo as u8 {
            // Process it as GameDataTo
            let inner_outgoing = handle_game_data(
                client,
                &mut inner_reader,
                MessageFlags::GameDataTo,
                game_manager,
                client_manager,
            )
            .await;
            outgoing.extend(inner_outgoing);
        }
    }

    outgoing
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Get all connections for players in a game.
fn get_game_connections(
    game: &railway_game_logic::Game,
    client_manager: &ClientManager,
) -> Vec<Arc<Connection>> {
    game.players
        .iter()
        .filter_map(|p| {
            client_manager
                .get(p.client_id)
                .map(|c| c.connection.clone())
        })
        .collect()
}

/// Build a GameData(SpawnFlag) message that spawns a bare/blank
/// `PlayerInfo` object (spawn type 11, server-owned) for `owning_client_id`.
///
/// This matches C#'s `SpawnPlayerInfoAsync` + `InnerPlayerInfo.SerializeAsync`
/// field-for-field:
/// - `PlayerId`: byte (NOT packed)
/// - `ClientId`: packed i32
/// - `Outfits.Count` (byte) + for each: outfit key (byte) + `PlayerOutfit.Serialize`
///   (PlayerName, Color(packed i32), HatId/PetId/SkinId/VisorId/NamePlateId
///   strings, then 5 sequence-id bytes)
/// - `PlayerLevel`: packed i32
/// - flags byte (disconnected/dead bits)
/// - `RoleType`: u16
/// - `RoleWhenAlive.HasValue` bool (+ u16 if true)
/// - `Tasks.Count`: byte
/// - `FriendCode` / `PUID`: strings
///
/// Everything is left blank/default — the owning client fills in its own
/// name/color/cosmetics afterward via RPCs, exactly like a real client
/// would after receiving this from the real server.
/// Build the raw SpawnFlag payload (objectId..last component, matching
/// what `sub_reader.full_buffer()` captures for client-sent spawns) for a
/// server-generated `PlayerInfo` object.
///
/// IMPORTANT: real C#'s `SpawnPlayerInfoAsync` sets
/// `playerInfo.OwnerId = ServerOwned` where `ServerOwned = -4` (see
/// `Game.Data.cs`'s constants — NOT -2, which is a DIFFERENT sentinel
/// ("InvalidClient") used only for the HOST CLIENT's own global spawns
/// like VoteBanSystem/NormalGameManager/LobbyBehaviour). A previous
/// version of this function used -2 here, which doesn't match what a
/// real server sends for its own PlayerInfo spawns.
fn build_player_info_spawn_payload(
    net_id: railway_protocol::NetId,
    owning_client_id: i32,
    player_id_slot: u8,
) -> bytes::Bytes {
    let mut w = MessageWriter::new(SendOption::Reliable);
    w.write_packed_u32(11); // objectId: PlayerInfo
    w.write_packed_i32(-4); // ownerId: ServerOwned (see doc comment above)
    w.write_byte(0); // spawnFlags: None
    w.write_packed_i32(1); // componentsCount: PlayerInfo has no children
    w.write_packed_u32(net_id);

    w.start_message(1); // component sub-message (initial state)
    // -- InnerPlayerInfo.SerializeAsync --
    w.write_byte(player_id_slot); // PlayerId
    w.write_packed_i32(owning_client_id); // ClientId
    w.write_byte(1); // Outfits.Count
    w.write_byte(0); // outfit key: PlayerOutfitType.Default
    w.write_string(""); // PlayerName (client sets via RPC)
    w.write_packed_i32(0); // Color
    w.write_string("missing"); // HatId
    w.write_string("missing"); // PetId
    w.write_string("missing"); // SkinId
    w.write_string("missing"); // VisorId
    w.write_string("missing"); // NamePlateId
    w.write_byte(0); // HatSequenceId
    w.write_byte(0); // PetSequenceId
    w.write_byte(0); // SkinSequenceId
    w.write_byte(0); // VisorSequenceId
    w.write_byte(0); // NamePlateSequenceId
    w.write_packed_i32(1); // PlayerLevel
    w.write_byte(0); // flags: not disconnected, not dead
    w.write_u16(0); // RoleType
    w.write_bool(false); // RoleWhenAlive.HasValue
    w.write_byte(0); // Tasks.Count
    w.write_string(""); // FriendCode
    w.write_string(""); // PUID
    w.end_message();

    w.into_bytes()
}

/// Wrap a raw SpawnFlag payload (as produced by
/// `build_player_info_spawn_payload`, or cached from an earlier spawn) in
/// a full `GameData(SpawnFlag)` message ready to send.
fn wrap_spawn_flag_message(game_code: GameCode, raw_spawn_payload: &[u8]) -> MessageWriter {
    let mut w = MessageWriter::new(SendOption::Reliable);
    w.start_message(MessageFlags::GameData as u8);
    w.write_i32(game_code);
    w.start_message(GameDataTag::SpawnFlag as u8);
    w.write_raw(raw_spawn_payload);
    w.end_message();
    w.end_message();
    w
}
