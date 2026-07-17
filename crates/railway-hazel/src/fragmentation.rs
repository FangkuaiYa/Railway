//! Message fragmentation and reassembly.
//!
//! When a reliable message exceeds the fragment threshold, it is split into
//! multiple fragments. Each fragment carries a fragment ID (unique per message),
//! a fragment index, and the total number of fragments. The receiver reassembles
//! the fragments and delivers the complete message once all fragments arrive.

use std::collections::HashMap;
use std::time::Instant;

use bytes::{BufMut, Bytes, BytesMut};
use parking_lot::Mutex;
use tracing::{debug, trace};

use crate::constants;
use crate::send_option::SendOption;

/// Manages the reassembly of fragmented messages for a connection.
pub struct FragmentManager {
    /// Active reassembly buffers, keyed by fragment ID.
    assemblies: Mutex<HashMap<u8, FragmentAssembly>>,
}

impl FragmentManager {
    pub fn new() -> Self {
        Self {
            assemblies: Mutex::new(HashMap::new()),
        }
    }

    /// Process an incoming fragment packet.
    ///
    /// If this fragment completes the message, returns the reassembled data.
    /// Otherwise, returns `None` (more fragments are needed).
    ///
    /// The fragment packet format (after the Fragment send option byte):
    /// - fragment_id: u8
    /// - fragment_index: u8
    /// - total_fragments: u8
    /// - payload: remaining bytes
    pub fn process_fragment(&self, data: &[u8]) -> Option<Bytes> {
        if data.len() < 3 {
            debug!("Fragment packet too short: {} bytes", data.len());
            return None;
        }

        let fragment_id = data[0];
        let fragment_index = data[1];
        let total_fragments = data[2];
        let payload = &data[3..];

        if total_fragments == 0 || fragment_index >= total_fragments {
            debug!(
                "Invalid fragment: id={} index={} total={}",
                fragment_id, fragment_index, total_fragments
            );
            return None;
        }

        let mut assemblies = self.assemblies.lock();

        // Clean up stale assemblies (older than 30 seconds)
        assemblies.retain(|_, a| a.created_at.elapsed().as_secs() < 30);

        let assembly = assemblies
            .entry(fragment_id)
            .or_insert_with(|| FragmentAssembly::new(total_fragments));

        if assembly.total_fragments != total_fragments {
            debug!(
                "Fragment total mismatch for id={}: expected {} got {}",
                fragment_id, assembly.total_fragments, total_fragments
            );
            return None;
        }

        if assembly.received[fragment_index as usize] {
            trace!(
                "Duplicate fragment: id={} index={}",
                fragment_id,
                fragment_index
            );
            return None;
        }

        assembly.received[fragment_index as usize] = true;
        assembly.fragments[fragment_index as usize] = Some(Bytes::copy_from_slice(payload));
        assembly.received_count += 1;

        trace!(
            id = fragment_id,
            index = fragment_index,
            received = assembly.received_count,
            total = total_fragments,
            "received fragment"
        );

        if assembly.received_count == total_fragments {
            // All fragments received — reassemble
            let total_size: usize = assembly.fragments.iter().filter_map(|f| f.as_ref()).map(|f| f.len()).sum();
            let mut buf = BytesMut::with_capacity(total_size);

            for fragment in &assembly.fragments {
                if let Some(f) = fragment {
                    buf.put_slice(f);
                }
            }

            assemblies.remove(&fragment_id);
            debug!(
                id = fragment_id,
                total_size,
                "fragment reassembly complete"
            );
            Some(buf.freeze())
        } else {
            None
        }
    }

    /// Split a large message into fragments, returning the fragment packets.
    ///
    /// Each fragment packet consists of:
    /// - Fragment send option byte
    /// - fragment_id (u8)
    /// - fragment_index (u8)
    /// - total_fragments (u8)
    /// - message data chunk
    pub fn fragment_message(data: &[u8], fragment_id: u8) -> Vec<Bytes> {
        let max_payload = constants::MAX_FRAGMENT_SIZE - 3; // account for the 3 header bytes
        let total_fragments =
            (data.len() + max_payload - 1) / max_payload;

        assert!(
            total_fragments <= constants::MAX_FRAGMENTS as usize,
            "message too large to fragment: {} bytes, would need {} fragments",
            data.len(),
            total_fragments
        );

        let mut fragments = Vec::with_capacity(total_fragments);

        for i in 0..total_fragments {
            let start = i * max_payload;
            let end = std::cmp::min(start + max_payload, data.len());
            let chunk = &data[start..end];

            let packet_size = 1 + 3 + chunk.len();
            let mut buf = BytesMut::with_capacity(packet_size);
            buf.put_u8(SendOption::Fragment.to_byte());
            buf.put_u8(fragment_id);
            buf.put_u8(i as u8);
            buf.put_u8(total_fragments as u8);
            buf.put_slice(chunk);

            fragments.push(buf.freeze());
        }

        fragments
    }

    /// Removes any stale assemblies.
    pub fn cleanup(&self, max_age_secs: u64) {
        let mut assemblies = self.assemblies.lock();
        assemblies.retain(|_, a| a.created_at.elapsed().as_secs() < max_age_secs);
    }
}

/// An in-progress fragment reassembly.
struct FragmentAssembly {
    total_fragments: u8,
    fragments: Vec<Option<Bytes>>,
    received: Vec<bool>,
    received_count: u8,
    created_at: Instant,
}

impl FragmentAssembly {
    fn new(total_fragments: u8) -> Self {
        let count = total_fragments as usize;
        Self {
            total_fragments,
            fragments: vec![None; count],
            received: vec![false; count],
            received_count: 0,
            created_at: Instant::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fragment_roundtrip() {
        let original = vec![0xABu8; 3000]; // Large enough to need multiple fragments
        let fragments = FragmentManager::fragment_message(&original, 42);

        assert!(fragments.len() > 1);

        let manager = FragmentManager::new();
        let mut reassembled = None;

        for fragment in &fragments {
            // Skip the first byte (send option)
            let result = manager.process_fragment(&fragment[1..]);
            if result.is_some() {
                reassembled = result;
            }
        }

        assert!(reassembled.is_some());
        assert_eq!(reassembled.unwrap().as_ref(), original.as_slice());
    }

    #[test]
    fn test_fragment_out_of_order() {
        let original = vec![0xCDu8; 2500];
        let fragments = FragmentManager::fragment_message(&original, 7);
        let manager = FragmentManager::new();

        // Deliver in reverse order
        let mut result = None;
        for fragment in fragments.iter().rev() {
            if let Some(data) = manager.process_fragment(&fragment[1..]) {
                result = Some(data);
            }
        }

        assert!(result.is_some());
        assert_eq!(result.unwrap().as_ref(), original.as_slice());
    }
}
