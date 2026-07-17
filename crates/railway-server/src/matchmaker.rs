//! Matchmaker: UDP listener that accepts connections and routes messages.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use railway_hazel::connection::ConnectionEvent;
use railway_hazel::message::MessageReader;
use railway_hazel::UdpConnectionListener;
use railway_protocol::messages::c2s::handshake;
use tokio::sync::RwLock;
use tracing::{debug, error, info, trace, warn};

use crate::client::Client;
use crate::client_manager::ClientManager;
use crate::game_manager::GameManager;
use crate::message_router::{self, OutgoingMessage};

/// Map from connection endpoint to client ID for fast lookup.
type EndpointMap = Arc<RwLock<HashMap<SocketAddr, i32>>>;

/// Run the matchmaker: bind, start background tasks, process events.
pub async fn run_matchmaker(
    listen_addr: SocketAddr,
    client_manager: Arc<ClientManager>,
    game_manager: Arc<GameManager>,
) {
    let listener = match UdpConnectionListener::bind(listen_addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("failed to bind UDP listener on {}: {}", listen_addr, e);
            return;
        }
    };

    let local_addr = listener.local_addr();
    let endpoint_map: EndpointMap = Arc::new(RwLock::new(HashMap::new()));

    // Start background tasks (UDP recv + tick)
    listener.start();

    info!("matchmaker listening on {}", local_addr);

    // Main event loop
    let mut shutdown = false;
    while !shutdown {
        match listener.recv().await {
            Some(event) => {
                // Run the actual event handling inside a spawned task and
                // await it immediately. This preserves strict in-order
                // processing (we don't move on to the next event until this
                // one finishes) while isolating panics: if a malformed /
                // unexpected packet triggers a bug in parsing somewhere
                // deep in handshake/message routing, `tokio::spawn` catches
                // the unwind and hands it back as a `JoinError` instead of
                // taking down this whole loop (and therefore the whole
                // server) with it. Previously an attacker — or just a
                // client using a slightly different protocol revision —
                // could crash the entire matchmaker with a single UDP
                // packet.
                let em = Arc::clone(&endpoint_map);
                let cm = Arc::clone(&client_manager);
                let gm = Arc::clone(&game_manager);
                let handle = tokio::spawn(async move {
                    handle_connection_event(event, em, cm, gm).await;
                });

                if let Err(join_err) = handle.await {
                    error!(
                        "connection event handler panicked — packet ignored, server stays up: {}",
                        join_err
                    );
                }
            }
            None => {
                // Channel closed — listener shut down
                info!("listener event channel closed, shutting down matchmaker");
                shutdown = true;
            }
        }
    }
}

/// Handle a single connection event. Spawned as its own task by the caller
/// so a panic here can't bring down the whole matchmaker loop.
async fn handle_connection_event(
    event: ConnectionEvent,
    endpoint_map: EndpointMap,
    client_manager: Arc<ClientManager>,
    game_manager: Arc<GameManager>,
) {
    match event {
        ConnectionEvent::Connected { connection, handshake_data } => {
            let endpoint = connection.endpoint();

            // Parse handshake
            let mut reader = MessageReader::new(handshake_data, 0);
            let hs = handshake::deserialize_handshake(&mut reader);

            info!(
                "new connection from {}: name='{}' version={}",
                endpoint, hs.name, hs.client_version,
            );

            // Parse Reactor mod data (before creating client)
            let reactor_mods: Option<Vec<crate::reactor::ReactorMod>> = hs.trailing_mod_data.as_ref()
                .and_then(|data| crate::reactor::parse_reactor_handshake(data))
                .map(|r| r.mods);

            // Version check
            match client_manager.check_version(&hs) {
                crate::client_manager::VersionCheck::Accept => {}
                crate::client_manager::VersionCheck::Reject(msg) => {
                    connection.reject_hello(msg);
                    return;
                }
            }

            // Name check
            if !client_manager.check_name(&hs.name) {
                connection.reject_hello("Invalid username");
                return;
            }

            // Accept and register
            connection.accept_hello();

            let id = client_manager.next_id();
            let client = Arc::new(Client::new(id, connection.clone(), hs, reactor_mods));
            let client_name = client.name().to_string();

            // Log Reactor mods with player name+id (before moving into register)
            if let Some(mods) = &client.reactor_mods {
                if mods.is_empty() {
                    debug!("{}[{}] vanilla client", client_name, id);
                } else {
                    info!("{}[{}] Reactor mods ({}):", client_name, id, mods.len());
                    for m in mods {
                        let flags = if m.is_required_on_all_clients() { " ALL" } else { "" };
                        info!("{}[{}]   {} v{}{}", client_name, id, m.id, m.version, flags);
                    }
                }
            }

            client_manager.register(client.clone());

            // Track endpoint → client ID mapping
            endpoint_map.write().await.insert(endpoint, id);

            info!("client {} ({}) connected from {}", id, client_name, endpoint);
        }

        ConnectionEvent::DataReceived { connection, mut reader } => {
            let endpoint = connection.endpoint();

            // Look up client by endpoint
            let client_id = {
                let map = endpoint_map.read().await;
                map.get(&endpoint).copied()
            };

            let client = match client_id.and_then(|id| client_manager.get(id)) {
                Some(c) => c,
                None => {
                    warn!("data from unknown client {} — disconnecting", endpoint);
                    connection.disconnect(Some("not registered"));
                    return;
                }
            };

            if !connection.is_connected() {
                return;
            }

            // Parse individual Hazel messages from the raw data.
            // Each message is: [len: u16][tag: u8][payload...]
            // A UDP packet may contain multiple messages.
            trace!("DataReceived from {}: {} bytes remaining", endpoint, reader.remaining());
            while reader.remaining() > 0 {
                if let Some(mut message) = reader.read_message() {
                    debug!("message tag=0x{:02X} len={}", message.tag, message.len());

                    // Anti-cheat: reject implausibly large sub-messages.
                    // This check existed in the anticheat module, and
                    // config.toml already had `enable_packet_size_checks`
                    // / `packet_size_limit` fields for it, but neither was
                    // ever actually consulted anywhere — the check simply
                    // wasn't wired up at all.
                    if client_manager.anticheat.enabled
                        && client_manager.anticheat.enable_packet_size_checks
                    {
                        let size_check = railway_game_logic::anticheat_checks::check_packet_size(
                            message.len(),
                            client_manager.anticheat.packet_size_limit,
                        );
                        if size_check.is_cheat() {
                            warn!(
                                "client {} sent oversized message (tag=0x{:02X}): {}",
                                client.id,
                                message.tag,
                                size_check.cheat_message().unwrap_or("")
                            );
                            break;
                        }
                    }

                    // Route each message
                    let outgoing = message_router::route_message(
                        &client,
                        &mut message,
                        &client_manager,
                        &game_manager,
                    ).await;

                    // Process outgoing messages
                    process_outgoing(outgoing);
                } else {
                    break;
                }

                if !connection.is_connected() {
                    break;
                }
            }
        }

        ConnectionEvent::Disconnected { connection, reason } => {
            let endpoint = connection.endpoint();

            // Look up and remove client
            let client_id = {
                let mut map = endpoint_map.write().await;
                map.remove(&endpoint)
            };

            if let Some(client_id) = client_id {
                if let Some(client) = client_manager.get(client_id) {
                    // Remove from game if in one
                    if let Some(game_code) = client.game_code() {
                        if let Some(game) = game_manager.find(game_code) {
                            let _ = game.remove_player(
                                client_id,
                                railway_protocol::DisconnectReason::ExitGame,
                            );

                            // Matches C#'s Message04RemovePlayerS2C: the
                            // game must broadcast the departure (and the
                            // possibly-migrated new host id) to every
                            // OTHER player still in the game. This was
                            // completely missing before — remove_player()
                            // only updated server-side bookkeeping, so
                            // remaining clients never learned someone
                            // left (or that the host changed), and their
                            // player list/host state just froze at
                            // whatever it was before the disconnect.
                            if game.player_count() > 0 {
                                let new_host_id = game.host_id();
                                let remaining: Vec<Arc<railway_hazel::connection::Connection>> = game
                                    .players
                                    .iter()
                                    .filter_map(|p| client_manager.get(p.client_id).map(|c| c.connection.clone()))
                                    .collect();

                                if !remaining.is_empty() {
                                    let mut writer = railway_hazel::MessageWriter::new(railway_hazel::SendOption::Reliable);
                                    railway_protocol::messages::s2c::remove_player::serialize(
                                        &mut writer,
                                        true,
                                        game_code,
                                        client_id,
                                        new_host_id,
                                        railway_protocol::DisconnectReason::ExitGame,
                                    );
                                    let bytes = writer.into_bytes();
                                    for conn in &remaining {
                                        let _ = conn.send_data(bytes.clone(), railway_hazel::SendOption::Reliable);
                                    }
                                    info!(
                                        "broadcast RemovePlayer client={} new_host={} to {} remaining players",
                                        client_id,
                                        new_host_id,
                                        remaining.len()
                                    );
                                }
                            }

                            if game.player_count() == 0 {
                                game_manager.remove(game_code);
                            }
                        }
                    }

                    client.dispose();
                }
                client_manager.remove(client_id);
                info!("client {} disconnected: {}", client_id, reason);
            }
        }
    }
}

fn process_outgoing(outgoing: Vec<OutgoingMessage>) {
    debug!("sending {} outgoing message(s)", outgoing.len());
    for msg in outgoing {
        match msg {
            OutgoingMessage::Send { connection, writer } => {
                let bytes = writer.into_bytes();
                // Extract the inner message tag for a human-readable label
                let label = if bytes.len() >= 3 {
                    let len = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
                    let tag = bytes.get(2).copied().unwrap_or(0);
                    format!("flag=0x{tag:02X} len={len}")
                } else {
                    format!("{} bytes", bytes.len())
                };
                debug!("  -> send {} to {}", label, connection.endpoint());
                let _ = connection.send_data(bytes, railway_hazel::SendOption::Reliable);
            }
            OutgoingMessage::Broadcast { connections, writer } => {
                let data = writer.into_bytes();
                let label = if data.len() >= 3 {
                    let len = u16::from_le_bytes([data[0], data[1]]) as usize;
                    let tag = data.get(2).copied().unwrap_or(0);
                    format!("flag=0x{tag:02X} len={len}")
                } else {
                    format!("{} bytes", data.len())
                };
                for conn in &connections {
                    let _ = conn.send_data(data.clone(), railway_hazel::SendOption::Reliable);
                }
                debug!("  -> broadcast {} to {} conn(s)", label, connections.len());
            }
            OutgoingMessage::Disconnect { connection, reason, message } => {
                info!("  -> Disconnect {}: {:?} - {}", connection.endpoint(), reason, message);
                let _ = connection.disconnect(Some(&message));
            }
        }
    }
}
