//! Connection: a single Hazel UDP connection. Always `Arc<Connection>`.
//!
//! Matches the real C# UdpConnection implementation:
//! - Sequence IDs are BIG-endian u16
//! - ACK packets: [0x0A][id_high][id_low][1-byte bitmap]
//! - Hello/Ping use reliable mechanism (get ACK'd, measure RTT)
//! - First hello triggers Connected event with handshake data

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, info, trace, warn};

use crate::constants;
use crate::error::HazelError;
use crate::fragmentation::FragmentManager;
use crate::message::MessageReader;
use crate::reliability::{self, ReceiveReliability, SendReliability};
use crate::send_option::SendOption;
use crate::state::ConnectionState;
use crate::HazelResult;

#[derive(Debug)]
pub enum ConnectionEvent {
    Connected { connection: Arc<Connection>, handshake_data: Bytes },
    DataReceived { connection: Arc<Connection>, reader: MessageReader },
    Disconnected { connection: Arc<Connection>, reason: String },
}

pub struct Connection {
    pub(crate) endpoint: SocketAddr,
    pub(crate) state: Mutex<ConnectionState>,
    pub(crate) send_rel: SendReliability,
    pub(crate) recv_rel: ReceiveReliability,
    pub(crate) fragment_mgr: FragmentManager,
    pub(crate) outbound_tx: mpsc::UnboundedSender<(SocketAddr, Bytes)>,
    pub(crate) inbound_tx: mpsc::UnboundedSender<ConnectionEvent>,
    pub(crate) ping_ms: AtomicU32,
    pub(crate) last_recv: Mutex<Instant>,
    pub(crate) disposed: AtomicBool,
    pub(crate) is_first: AtomicBool,
}

impl Connection {
    pub(crate) fn new_arc(
        endpoint: SocketAddr,
        outbound_tx: mpsc::UnboundedSender<(SocketAddr, Bytes)>,
        inbound_tx: mpsc::UnboundedSender<ConnectionEvent>,
    ) -> Arc<Self> {
        Arc::new(Self {
            endpoint,
            state: Mutex::new(ConnectionState::HelloReceived),
            send_rel: SendReliability::new(),
            recv_rel: ReceiveReliability::new(),
            fragment_mgr: FragmentManager::new(),
            outbound_tx,
            inbound_tx,
            ping_ms: AtomicU32::new(f32::to_bits(500.0)),
            last_recv: Mutex::new(Instant::now()),
            disposed: AtomicBool::new(false),
            is_first: AtomicBool::new(true),
        })
    }

    pub fn endpoint(&self) -> SocketAddr { self.endpoint }
    pub fn state(&self) -> ConnectionState { *self.state.lock() }
    pub fn is_connected(&self) -> bool { self.state() == ConnectionState::Connected }
    pub fn average_ping_ms(&self) -> f32 { f32::from_bits(self.ping_ms.load(Ordering::Relaxed)) }

    pub fn accept_hello(&self) {
        let mut s = self.state.lock();
        if *s == ConnectionState::HelloReceived {
            *s = ConnectionState::Connected;
            info!(endpoint = %self.endpoint, "accepted");
        }
    }

    pub fn reject_hello(self: &Arc<Self>, reason: &str) {
        let mut s = self.state.lock();
        if *s != ConnectionState::HelloReceived { return; }
        *s = ConnectionState::Disconnecting;
        drop(s);
        let pkt = reliability::build_disconnect_packet(Some(reason));
        let _ = self.outbound_tx.send((self.endpoint, pkt));
        *self.state.lock() = ConnectionState::Disconnected;
        info!(endpoint = %self.endpoint, reason, "rejected");
        let _ = self.inbound_tx.send(ConnectionEvent::Disconnected {
            connection: Arc::clone(self), reason: reason.to_string(),
        });
    }

    pub fn send_data(&self, data: Bytes, option: SendOption) -> HazelResult<()> {
        if self.disposed.load(Ordering::Relaxed) {
            return Err(HazelError::CannotSend { state: "disposed".into() });
        }
        let state = self.state();
        if !state.can_send() {
            return Err(HazelError::CannotSend { state: state.to_string() });
        }
        match option {
            SendOption::Reliable => {
                if data.len() > constants::FRAGMENT_THRESHOLD {
                    let fid = self.send_rel.next_fragment_id();
                    for frag in FragmentManager::fragment_message(&data, fid) {
                        let _ = self.outbound_tx.send((self.endpoint, frag));
                    }
                } else {
                    let id = self.send_rel.enqueue(data.clone());
                    let pkt = reliability::build_reliable_packet(SendOption::Reliable.to_byte(), id, &data);
                    let _ = self.outbound_tx.send((self.endpoint, pkt));
                }
            }
            SendOption::Unreliable => {
                let pkt = reliability::build_unreliable_packet(SendOption::Unreliable.to_byte(), &data);
                let _ = self.outbound_tx.send((self.endpoint, pkt));
            }
            _ => return Err(HazelError::CannotSend { state: format!("{:?}", option) }),
        }
        Ok(())
    }

    pub fn send_writer(&self, writer: crate::message::MessageWriter) -> HazelResult<()> {
        let option = writer.send_option();
        self.send_data(writer.into_bytes(), option)
    }

    pub fn disconnect(self: &Arc<Self>, reason: Option<&str>) {
        let mut s = self.state.lock();
        if matches!(*s, ConnectionState::Disconnected | ConnectionState::Disconnecting) { return; }
        *s = ConnectionState::Disconnecting;
        drop(s);
        let pkt = reliability::build_disconnect_packet(reason);
        let _ = self.outbound_tx.send((self.endpoint, pkt));
        *self.state.lock() = ConnectionState::Disconnected;
        let r = reason.unwrap_or("Disconnected").to_string();
        let _ = self.inbound_tx.send(ConnectionEvent::Disconnected {
            connection: Arc::clone(self), reason: r,
        });
    }

    pub fn dispose(&self) {
        self.disposed.store(true, Ordering::SeqCst);
        self.send_rel.clear();
    }

    // ---- Packet processing (called from listener with &Arc<Connection>) ----

    /// Process an incoming UDP packet. `conn` must be the Arc<Connection> from the listener registry.
    pub(crate) fn on_packet(conn: &Arc<Self>, data: &[u8]) {
        if conn.disposed.load(Ordering::Relaxed) || data.is_empty() { return; }
        *conn.last_recv.lock() = Instant::now();

        let send_byte = data[0];
        trace!(endpoint = %conn.endpoint, send_byte, len = data.len(), "packet recv");

        // First packet detection
        if conn.is_first.swap(false, Ordering::Relaxed) {
            if send_byte == SendOption::Hello.to_byte() {
                let handshake = if data.len() > 4 {
                    Bytes::copy_from_slice(&data[4..])
                } else {
                    Bytes::new()
                };
                debug!(
                    endpoint = %conn.endpoint,
                    len = handshake.len(),
                    "handshake received"
                );
                let _ = conn.inbound_tx.send(ConnectionEvent::Connected {
                    connection: Arc::clone(conn),
                    handshake_data: handshake,
                });
            }
        }

        match send_byte {
            b if b == SendOption::Acknowledgment.to_byte() => {
                if data.len() < 3 { return; }
                let id = reliability::parse_id(data, 1);
                let cur_ping = conn.average_ping_ms();
                let _ = conn.send_rel.ack(id, cur_ping);
                if let Some(new_ping) = conn.send_rel.ack(id, cur_ping) {
                    conn.ping_ms.store(f32::to_bits(new_ping), Ordering::Relaxed);
                }
                if data.len() >= 4 {
                    let mut bitmap = data[3];
                    for i in 0..8u16 {
                        if bitmap & 1 != 0 {
                            let _ = conn.send_rel.ack(id.wrapping_sub(1 + i), cur_ping);
                        }
                        bitmap >>= 1;
                    }
                }
            }
            b if b == SendOption::Hello.to_byte() || b == SendOption::Ping.to_byte() => {
                if data.len() >= 3 {
                    let id = reliability::parse_id(data, 1);
                    conn.recv_rel.record(id);
                    conn.send_ack(id);
                    trace!(endpoint = %conn.endpoint, id, "ACK ping");
                }
            }
            b if b == SendOption::Reliable.to_byte() => {
                if data.len() < 3 { return; }
                let id = reliability::parse_id(data, 1);
                if conn.recv_rel.record(id) {
                    conn.send_ack(id);
                    trace!(endpoint = %conn.endpoint, id, "ACK reliable");
                    Connection::deliver_data(Arc::clone(conn), &data[3..]);
                }
            }
            b if b == SendOption::Disconnect.to_byte() => {
                let reason = data.get(1..)
                    .map(|b| String::from_utf8_lossy(b).to_string())
                    .unwrap_or_else(|| "The remote sent a disconnect request".into());
                Connection::on_disconnect(conn, &reason);
            }
            b if b == SendOption::Fragment.to_byte() => {
                if let Some(reassembled) = conn.fragment_mgr.process_fragment(&data[1..]) {
                    Connection::deliver_data(Arc::clone(conn), &reassembled);
                }
            }
            _ => {
                Connection::deliver_data(Arc::clone(conn), &data[1..]);
            }
        }
    }

    pub(crate) fn tick(conn: &Arc<Self>, now: Instant) {
        if conn.disposed.load(Ordering::Relaxed) { return; }
        let state = conn.state();
        if !state.is_alive() { return; }

        let last = *conn.last_recv.lock();
        if now.duration_since(last) > constants::CONNECTION_TIMEOUT {
            warn!(endpoint = %conn.endpoint, "timed out");
            return Connection::on_disconnect(conn, "Connection timed out");
        }

        if state == ConnectionState::Connected {
            let ping = conn.average_ping_ms();
            for p in conn.send_rel.get_resends(ping, constants::DISCONNECT_TIMEOUT_MS) {
                let pkt = reliability::build_reliable_packet(
                    SendOption::Reliable.to_byte(), p.id, &p.data,
                );
                let _ = conn.outbound_tx.send((conn.endpoint, pkt));
            }
        }
        conn.fragment_mgr.cleanup(30);
    }

    // ---- Private helpers ----

    fn send_ack(&self, id: u16) {
        let bitmap = self.recv_rel.build_ack_bitmap(id);
        let ack = reliability::build_ack_packet(id, bitmap);
        let _ = self.outbound_tx.send((self.endpoint, ack));
    }

    fn deliver_data(conn: Arc<Self>, payload: &[u8]) {
        if payload.is_empty() { return; }
        let reader = MessageReader::new(Bytes::copy_from_slice(payload), 0);
        let _ = conn.inbound_tx.send(ConnectionEvent::DataReceived {
            connection: Arc::clone(&conn),
            reader,
        });
    }

    fn on_disconnect(conn: &Arc<Self>, reason: &str) {
        let mut s = conn.state.lock();
        if matches!(*s, ConnectionState::Disconnected) { return; }
        *s = ConnectionState::Disconnected;
        drop(s);
        info!(endpoint = %conn.endpoint, reason, "disconnected");
        let _ = conn.inbound_tx.send(ConnectionEvent::Disconnected {
            connection: Arc::clone(conn),
            reason: reason.to_string(),
        });
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.dispose();
    }
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("endpoint", &self.endpoint)
            .field("state", &self.state())
            .field("ping_ms", &self.average_ping_ms())
            .finish()
    }
}

unsafe impl Send for Connection {}
unsafe impl Sync for Connection {}
