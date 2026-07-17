//! UDP connection listener. Binds a socket, manages connections, dispatches packets.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use dashmap::DashMap;
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Mutex};
use tokio::time;
use tracing::{debug, error, info, trace, warn};

use crate::connection::{Connection, ConnectionEvent};
use crate::send_option::SendOption;

pub struct UdpConnectionListener {
    socket: Arc<UdpSocket>,
    connections: Arc<DashMap<SocketAddr, Arc<Connection>>>,
    outbound_tx: mpsc::UnboundedSender<(SocketAddr, Bytes)>,
    event_tx: mpsc::UnboundedSender<ConnectionEvent>,
    event_rx: Mutex<mpsc::UnboundedReceiver<ConnectionEvent>>,
    local_addr: SocketAddr,
}

impl UdpConnectionListener {
    pub async fn bind(addr: SocketAddr) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(addr).await?;
        let local_addr = socket.local_addr()?;
        let socket = Arc::new(socket);

        let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<(SocketAddr, Bytes)>();
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        // Spawn outbound send task
        let send_socket = Arc::clone(&socket);
        tokio::spawn(async move {
            while let Some((dst, data)) = outbound_rx.recv().await {
                if let Err(e) = send_socket.send_to(&data, dst).await {
                    warn!("send error to {dst}: {e}");
                }
                trace!(dst = %dst, len = data.len(), "sent");
            }
        });

        info!("Hazel listener bound to {local_addr}");

        Ok(Self {
            socket,
            connections: Arc::new(DashMap::new()),
            outbound_tx,
            event_tx,
            event_rx: Mutex::new(event_rx),
            local_addr,
        })
    }

    pub fn local_addr(&self) -> SocketAddr { self.local_addr }

    /// Returns a sender for connection events (cloned for handlers).
    pub fn event_sender(&self) -> mpsc::UnboundedSender<ConnectionEvent> {
        self.event_tx.clone()
    }

    /// Consume the next connection event. Non-blocking — returns None if no event is ready.
    /// Use in a tokio::select! loop or call from an async context with `.await`.
    pub async fn recv(&self) -> Option<ConnectionEvent> {
        self.event_rx.lock().await.recv().await
    }

    /// Start background tasks: tick loop + UDP receive loop.
    /// Returns immediately so the caller can call `recv()` to process events.
    pub fn start(&self) {
        let connections = Arc::clone(&self.connections);
        let outbound_tx = self.outbound_tx.clone();
        let event_tx = self.event_tx.clone();
        let socket = Arc::clone(&self.socket);

        // Spawn tick task
        let tick_conns = Arc::clone(&connections);
        let tick_evt = event_tx.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(50));
            loop {
                interval.tick().await;
                let now = Instant::now();
                tick_conns.retain(|_addr, conn| {
                    Connection::tick(conn, now);
                    let alive = conn.state().is_alive();
                    if !alive {
                        let _ = tick_evt.send(ConnectionEvent::Disconnected {
                            connection: Arc::clone(conn),
                            reason: "timed out".into(),
                        });
                    }
                    alive
                });
            }
        });

        // Spawn UDP receive task
        tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];
            debug!("UDP recv task started");
            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((n, src)) => {
                        let data = Bytes::copy_from_slice(&buf[..n]);
                        let send_byte = data.first().copied().unwrap_or(0);
                        // Skip logging ping packets (0x0C) and pure ACKs (0x0A) — they spam the log
                        if send_byte != 0x0C && send_byte != 0x0A {
                            debug!("UDP recv {} bytes from {src}, type=0x{send_byte:02X}", n);
                        } else {
                            trace!("UDP recv {} bytes from {src}, type=0x{send_byte:02X}", n);
                        }

                        // Isolate panics per-packet: a single malformed/unexpected
                        // packet must never be able to kill this whole receive loop
                        // (which would silently stop the server from accepting any
                        // further UDP traffic).
                        let connections = Arc::clone(&connections);
                        let event_tx = event_tx.clone();
                        let outbound_tx = outbound_tx.clone();
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            Self::dispatch(&connections, &event_tx, &outbound_tx, src, data);
                        }));
                        if let Err(e) = result {
                            let msg = e
                                .downcast_ref::<&str>()
                                .map(|s| s.to_string())
                                .or_else(|| e.downcast_ref::<String>().cloned())
                                .unwrap_or_else(|| "<non-string panic payload>".to_string());
                            error!(
                                "panic while dispatching packet from {src} — packet dropped, listener stays up: {msg}"
                            );
                        }
                    }
                    Err(e) => {
                        error!("UDP recv fatal: {e}");
                        break;
                    }
                }
            }
            error!("UDP recv task EXITED!");
        });

        info!("listener background tasks started");
    }

    fn dispatch(
        connections: &DashMap<SocketAddr, Arc<Connection>>,
        event_tx: &mpsc::UnboundedSender<ConnectionEvent>,
        outbound_tx: &mpsc::UnboundedSender<(SocketAddr, Bytes)>,
        src: SocketAddr,
        data: Bytes,
    ) {
        if data.is_empty() { return; }

        let send_byte = data[0];

        if send_byte == SendOption::Hello.to_byte() {
            if let Some(conn) = connections.get(&src) {
                Connection::on_packet(&conn, &data);
                return;
            }
            let conn = Connection::new_arc(src, outbound_tx.clone(), event_tx.clone());
            connections.insert(src, Arc::clone(&conn));
            Connection::on_packet(&conn, &data);
            return;
        }

        if let Some(conn) = connections.get(&src) {
            Connection::on_packet(&conn, &data);
        } else {
            trace!("packet from unknown {src}");
        }
    }
}
