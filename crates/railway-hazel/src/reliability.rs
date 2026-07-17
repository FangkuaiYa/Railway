//! Reliability layer matching the real Hazel C# implementation.
//!
//! Key details from the C# source:
//! - Sequence numbers are 2-byte BIG-endian (u16, wrapping)
//! - ACK packet: [type=0x0A][id_high][id_low][1-byte bitmap]
//! - Bitmap bit i (0..7) = 1 if packet (id - 1 - i) was received
//! - Missing packets tracked in a HashSet<ushort>
//! - Ping: EMA with _pingMs = max(50, _pingMs * 0.7 + rtt * 0.3)
//! - Resend timeout: ping * 2 (capped at 300ms for first, doubled per resend, capped at 1000ms)
//! - Disconnect timeout: 5000ms total lifetime

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Instant;

use bytes::{BufMut, Bytes, BytesMut};
use parking_lot::Mutex;
use tracing::{debug, trace, warn};

use crate::send_option::SendOption;

// ---- Send side ----

pub struct SendReliability {
    next_id: AtomicU16,
    packets: Mutex<VecDeque<ReliablePacket>>,
    /// For generating unique fragment IDs (from the u16 seq space upper bits).
    fragment_counter: AtomicU16,
}

impl SendReliability {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU16::new(0),
            packets: Mutex::new(VecDeque::with_capacity(256)),
            fragment_counter: AtomicU16::new(0),
        }
    }

    pub fn next_sequence(&self) -> u16 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn enqueue(&self, data: Bytes) -> u16 {
        let id = self.next_sequence();
        let mut packets = self.packets.lock();
        packets.push_back(ReliablePacket {
            id,
            data,
            retransmissions: 0,
            next_timeout: 0, // set on first send
            stopwatch: Instant::now(),
        });
        id
    }

    /// Mark a packet as acknowledged by ID. Returns the RTT if found.
    pub fn ack(&self, id: u16, current_ping: f32) -> Option<f32> {
        let mut packets = self.packets.lock();
        if let Some(pos) = packets.iter().position(|p| p.id == id) {
            let pkt = packets.remove(pos).unwrap();
            let rtt = pkt.stopwatch.elapsed().as_secs_f32() * 1000.0;
            // EMA: ping = max(50, ping * 0.7 + rtt * 0.3)
            let new_ping = (current_ping * 0.7 + rtt * 0.3).max(50.0);
            trace!(id, rtt, new_ping, "acked");
            Some(new_ping)
        } else {
            None
        }
    }

    /// Get packets that need resending. Returns (id, data, is_new_attempt).
    pub fn get_resends(&self, ping_ms: f32, disconnect_timeout_ms: u64) -> Vec<ResendPacket> {
        let mut packets = self.packets.lock();
        let now = Instant::now();
        let mut resends = Vec::new();
        let mut to_remove = Vec::new();

        for (idx, pkt) in packets.iter_mut().enumerate() {
            let lifetime = now.duration_since(pkt.stopwatch).as_millis() as u64;

            // Disconnect timeout
            if lifetime >= disconnect_timeout_ms {
                warn!(id = pkt.id, lifetime, "reliable packet timed out");
                to_remove.push(idx);
                continue;
            }

            // Lazily compute the real first-attempt deadline the first time
            // we see this packet, BEFORE checking due-ness.
            //
            // Bug this fixes: `next_timeout` starts at the sentinel value 0
            // (see `enqueue`). The old code checked
            // `lifetime >= pkt.next_timeout` FIRST and only computed the
            // real deadline *inside* that branch — but `0 >= 0` is always
            // true, so on the very first tick after ANY reliable packet was
            // sent (Hello, HostGame, JoinGame, ...), it was immediately
            // flagged as "due for resend" and a duplicate copy went out
            // over the wire before the real ACK even had a chance to
            // arrive. Every single reliable message during connect/join
            // was getting one guaranteed spurious retransmission — that's
            // extra round-trip work and duplicate-packet bookkeeping on
            // the client for literally no reason, which is exactly the
            // kind of "pointless verification/processing" adding latency
            // to joining a room.
            if pkt.next_timeout == 0 {
                pkt.next_timeout = ((ping_ms * 2.0) as u64).clamp(50, 300);
            }

            // Check if it's time to resend
            if lifetime >= pkt.next_timeout {
                pkt.retransmissions += 1;

                // Next attempt: double the previous timeout, capped at 1000ms.
                pkt.next_timeout = (pkt.next_timeout * 2).min(1000);

                resends.push(ResendPacket {
                    id: pkt.id,
                    data: pkt.data.clone(),
                });

                debug!(
                    id = pkt.id,
                    attempt = pkt.retransmissions,
                    "resending reliable packet"
                );
            }
        }

        // Remove timed-out packets (in reverse order to preserve indices)
        for idx in to_remove.into_iter().rev() {
            packets.remove(idx);
        }

        resends
    }

    pub fn clear(&self) {
        self.packets.lock().clear();
    }

    pub fn next_fragment_id(&self) -> u8 {
        (self.fragment_counter.fetch_add(1, Ordering::Relaxed) & 0xFF) as u8
    }
}

pub struct ResendPacket {
    pub id: u16,
    pub data: Bytes,
}

struct ReliablePacket {
    id: u16,
    data: Bytes,
    retransmissions: u32,
    next_timeout: u64, // ms
    stopwatch: Instant,
}

// ---- Receive side ----

pub struct ReceiveReliability {
    /// Last received reliable ID.
    last_received: Mutex<u16>,
    /// IDs we know we're missing.
    missing: Mutex<HashSet<u16>>,
    /// Whether we've received anything yet.
    initialized: Mutex<bool>,
}

impl ReceiveReliability {
    pub fn new() -> Self {
        Self {
            last_received: Mutex::new(0),
            missing: Mutex::new(HashSet::new()),
            initialized: Mutex::new(false),
        }
    }

    /// Process an incoming reliable packet ID.
    /// Returns true if this is a new (non-duplicate) packet.
    pub fn record(&self, id: u16) -> bool {
        let mut last = self.last_received.lock();
        let mut missing = self.missing.lock();
        let mut initialized = self.initialized.lock();

        if !*initialized {
            *last = id;
            *initialized = true;
            return true;
        }

        // Calculate overwrite pointer (last - 32768, wrapping)
        let overwrite = last.wrapping_sub(32768);

        // Determine if this ID is "new" (ahead of last, accounting for wrap)
        let is_new = if overwrite < *last {
            // Figure (2) from C#: new if id > last OR id <= overwrite
            id > *last || id <= overwrite
        } else {
            // Figure (3) from C#: new if id > last AND id <= overwrite
            id > *last && id <= overwrite
        };

        if is_new {
            // Mark all IDs between old last and new id as missing
            if id > *last {
                for i in last.wrapping_add(1)..id {
                    missing.insert(i);
                }
            } else {
                // Wrap-around: mark from last+1 to 65535 and 0 to id-1
                let cnt = (u16::MAX.wrapping_sub(*last)).wrapping_add(id) as usize;
                for i in 1..=cnt {
                    missing.insert(last.wrapping_add(i as u16));
                }
            }
            *last = id;
            true
        } else {
            // Not new — check if it was a missing packet
            missing.remove(&id)
        }
    }

    /// Build an ACK byte bitmap for the given received ID.
    /// Bit i (0..7) = 1 if packet (id - 1 - i) HAS been received (not missing).
    pub fn build_ack_bitmap(&self, id: u16) -> u8 {
        let missing = self.missing.lock();
        let mut bitmap: u8 = 0;
        for i in 0..8u16 {
            let check_id = id.wrapping_sub(1 + i);
            if !missing.contains(&check_id) {
                bitmap |= 1 << i;
            }
        }
        bitmap
    }

    pub fn last_received(&self) -> u16 {
        *self.last_received.lock()
    }
}

// ---- Packet builders (matching C# wire format) ----

/// Build a reliable data packet: [send_option: u8][id_high: u8][id_low: u8][data...]
/// Sequence numbers are BIG-endian!
pub fn build_reliable_packet(send_option: u8, id: u16, data: &[u8]) -> Bytes {
    let mut buf = BytesMut::with_capacity(3 + data.len());
    buf.put_u8(send_option);
    buf.put_u8((id >> 8) as u8); // big-endian high byte
    buf.put_u8(id as u8);        // big-endian low byte
    buf.put_slice(data);
    buf.freeze()
}

/// Build an unreliable data packet: [send_option: u8][data...]
pub fn build_unreliable_packet(send_option: u8, data: &[u8]) -> Bytes {
    let mut buf = BytesMut::with_capacity(1 + data.len());
    buf.put_u8(send_option);
    buf.put_slice(data);
    buf.freeze()
}

/// Build an ACK packet: [0x0A][id_high][id_low][bitmap]
pub fn build_ack_packet(id: u16, bitmap: u8) -> Bytes {
    let mut buf = BytesMut::with_capacity(4);
    buf.put_u8(SendOption::Acknowledgment.to_byte());
    buf.put_u8((id >> 8) as u8);
    buf.put_u8(id as u8);
    buf.put_u8(bitmap);
    buf.freeze()
}

/// Build a hello packet: [0x08][id_high][id_low][hazel_version: u8][handshake_data...]
pub fn build_hello_packet(id: u16, hazel_version: u8, handshake_data: &[u8]) -> Bytes {
    let mut buf = BytesMut::with_capacity(4 + handshake_data.len());
    buf.put_u8(SendOption::Hello.to_byte());
    buf.put_u8((id >> 8) as u8);
    buf.put_u8(id as u8);
    buf.put_u8(hazel_version);
    buf.put_slice(handshake_data);
    buf.freeze()
}

/// Build a disconnect packet: [0x09][optional_reason...]
pub fn build_disconnect_packet(reason: Option<&str>) -> Bytes {
    let reason_bytes = reason.map(|s| s.as_bytes()).unwrap_or(b"");
    let mut buf = BytesMut::with_capacity(1 + reason_bytes.len());
    buf.put_u8(SendOption::Disconnect.to_byte());
    if !reason_bytes.is_empty() {
        buf.put_slice(reason_bytes);
    }
    buf.freeze()
}

/// Build a ping packet: [0x0C][id_high][id_low]
pub fn build_ping_packet(id: u16) -> Bytes {
    let mut buf = BytesMut::with_capacity(3);
    buf.put_u8(SendOption::Ping.to_byte());
    buf.put_u8((id >> 8) as u8);
    buf.put_u8(id as u8);
    buf.freeze()
}

/// Parse a 2-byte big-endian sequence ID from a packet at the given offset.
pub fn parse_id(data: &[u8], offset: usize) -> u16 {
    ((data[offset] as u16) << 8) | (data[offset + 1] as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_big_endian_id() {
        let pkt = build_reliable_packet(1, 0x1234, &[0xAB]);
        // Byte 0: send option = 1
        // Byte 1: id high = 0x12
        // Byte 2: id low = 0x34
        assert_eq!(pkt[0], 1);
        assert_eq!(pkt[1], 0x12);
        assert_eq!(pkt[2], 0x34);
        assert_eq!(pkt[3], 0xAB);
        assert_eq!(parse_id(&pkt, 1), 0x1234);
    }

    #[test]
    fn test_ack_bitmap() {
        let recv = ReceiveReliability::new();
        recv.record(10);
        recv.record(12); // 11 is missing
        recv.record(13);

        // For id=13: bit0 = 12 received, bit1 = 11 missing, bit2 = 10 received
        let bitmap = recv.build_ack_bitmap(13);
        assert_eq!(bitmap & 0b001, 0b001); // bit0: 12 was received
        assert_eq!(bitmap & 0b010, 0);     // bit1: 11 was NOT received
        assert_eq!(bitmap & 0b100, 0b100); // bit2: 10 was received
    }

    #[test]
    fn test_receive_wrapping() {
        let recv = ReceiveReliability::new();
        assert!(recv.record(65530));
        assert!(recv.record(65535));
        assert!(recv.record(2)); // wraps around — should be new
        assert!(!recv.record(2)); // duplicate
        assert!(recv.record(1)); // missing packet, now filled
    }
}
