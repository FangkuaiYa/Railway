//! Redirect (flag 13) S2C message.

use railway_hazel::MessageWriter;
use crate::message_flags::MessageFlags;
use std::net::SocketAddr;

pub fn serialize(writer: &mut MessageWriter, addr: SocketAddr) {
    writer.start_message(MessageFlags::Redirect as u8);
    // Matches C# `IMessageWriter.Write(IPAddress)`, which writes the raw
    // 4 address bytes (no length prefix, no string encoding).
    let ip = match addr {
        SocketAddr::V4(v4) => v4.ip().octets(),
        SocketAddr::V6(_) => {
            // Among Us / Hazel only supports IPv4 redirects.
            [127, 0, 0, 1]
        }
    };
    writer.write_raw(&ip);
    writer.write_u16(addr.port());
    writer.end_message();
}
