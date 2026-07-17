//! Tests for the Hazel protocol implementation.

use super::*;
use bytes::Bytes;
use send_option::SendOption;

/// Test packed integer round-trips.
#[test]
fn test_packed_roundtrip() {
    use packed::{read_packed_i32, read_packed_u32, write_packed_i32, write_packed_u32};
    use bytes::BytesMut;

    let test_vals = [0u32, 1, 127, 128, 16383, 16384, 1_000_000, u32::MAX];
    for &v in &test_vals {
        let mut buf = BytesMut::new();
        write_packed_u32(&mut buf, v);
        let mut cursor = buf.freeze();
        assert_eq!(read_packed_u32(&mut cursor).unwrap(), v);
    }

    let test_ivals = [0i32, 1, -1, 127, -128, 16383, -16384, i32::MAX, i32::MIN];
    for &v in &test_ivals {
        let mut buf = BytesMut::new();
        write_packed_i32(&mut buf, v);
        let mut cursor = buf.freeze();
        assert_eq!(read_packed_i32(&mut cursor).unwrap(), v);
    }
}

/// Test message nesting and round-tripping.
#[test]
fn test_message_nesting() {
    let mut writer = MessageWriter::new(SendOption::Reliable);

    writer.start_message(0x05);
    writer.write_byte(42);
    writer.start_message(0x01);
    writer.write_packed_u32(999);
    writer.write_string("test");
    writer.end_message();
    writer.end_message();

    let data = writer.into_bytes();
    let mut reader = MessageReader::new(data, 0xFF);

    let msg = reader.read_message().unwrap();
    assert_eq!(msg.tag, 0x05);

    let inner = reader.read_message().unwrap();
    assert_eq!(inner.tag, 0x01);
    assert_eq!(inner.read_packed_u32(), 999);
    assert_eq!(inner.read_string(), "test");
}

/// Test message string round-trip with Unicode.
#[test]
fn test_string_roundtrip_unicode() {
    let mut writer = MessageWriter::new(SendOption::Reliable);
    writer.start_message(0x01);
    writer.write_string("Hello 世界 🌍");
    writer.end_message();

    let data = writer.into_bytes();
    let mut reader = MessageReader::new(data, 0xFF);
    let sub = reader.read_message().unwrap();
    assert_eq!(sub.read_string(), "Hello 世界 🌍");
}

/// Test fragment and reassemble.
#[test]
fn test_fragmentation() {
    let payload = vec![0xABu8; 3000];
    let fragments = FragmentManager::fragment_message(&payload, 42);
    assert!(fragments.len() >= 3, "should need at least 3 fragments");

    let mgr = FragmentManager::new();
    let mut result = None;
    for f in &fragments {
        if let Some(data) = mgr.process_fragment(&f[1..]) {
            result = Some(data);
        }
    }
    assert!(result.is_some(), "should reassemble");
    assert_eq!(&result.unwrap()[..], &payload[..]);
}

/// Test fragment reassembly with out-of-order delivery.
#[test]
fn test_fragmentation_out_of_order() {
    let payload = vec![0xCDu8; 2500];
    let fragments = FragmentManager::fragment_message(&payload, 7);
    let mgr = FragmentManager::new();

    let mut result = None;
    // Deliver fragments in reverse order
    for f in fragments.iter().rev() {
        if let Some(data) = mgr.process_fragment(&f[1..]) {
            result = Some(data);
        }
    }

    assert!(result.is_some(), "should reassemble out-of-order");
    assert_eq!(&result.unwrap()[..], &payload[..]);
}

/// Test reliability ACK tracking.
#[test]
fn test_ack_tracking() {
    let recv = ReceiveReliability::new();

    assert!(recv.record(1));
    assert!(!recv.record(1)); // duplicate
    assert!(recv.record(2));
    assert!(recv.record(5)); // gap — should still accept

    let (last_seq, bitmap) = recv.build_ack();
    assert_eq!(last_seq, 5);
}

/// Test send reliability enqueue/dequeue.
#[test]
fn test_send_reliability() {
    let send = SendReliability::new();
    let data = Bytes::from_static(b"hello");
    let seq = send.enqueue(data).unwrap();
    send.ack(seq);

    // After ACK, no resends should be pending
    let resends = send.get_resends(std::time::Instant::now());
    assert!(resends.is_empty());
}

/// Test packet building functions.
#[test]
fn test_build_packets() {
    let hello = reliability::build_hello_packet(42, 0);
    assert_eq!(hello[0], SendOption::Hello.to_byte());
    assert_eq!(u16::from_le_bytes([hello[1], hello[2]]), 42);

    let disconnect = reliability::build_disconnect_packet(Some("bye"));
    assert_eq!(disconnect[0], SendOption::Disconnect.to_byte());
    assert_eq!(&disconnect[1..], b"bye");

    let ping = reliability::build_ping_packet(99);
    assert_eq!(ping[0], SendOption::Ping.to_byte());
    assert_eq!(u16::from_le_bytes([ping[1], ping[2]]), 99);
}

/// Test multiple messages within one packet.
#[test]
fn test_multiple_top_level_messages() {
    let mut writer = MessageWriter::new(SendOption::Reliable);
    writer.start_message(0x01);
    writer.write_byte(1);
    writer.end_message();
    writer.start_message(0x02);
    writer.write_byte(2);
    writer.end_message();
    writer.start_message(0x03);
    writer.write_string("three");
    writer.end_message();

    let data = writer.into_bytes();
    let mut reader = MessageReader::new(data, 0xFF);

    let m1 = reader.read_message().unwrap();
    assert_eq!(m1.tag, 0x01);
    assert_eq!(m1.buffer[0], 1);

    let m2 = reader.read_message().unwrap();
    assert_eq!(m2.tag, 0x02);
    assert_eq!(m2.buffer[0], 2);

    let m3 = reader.read_message().unwrap();
    assert_eq!(m3.tag, 0x03);
    assert_eq!(m3.read_string(), "three");
}
