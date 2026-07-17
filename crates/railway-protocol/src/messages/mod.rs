//! Message serialization and deserialization for all Among Us protocol messages.
//!
//! Messages are organized into:
//! - `c2s`: Client-to-server messages
//! - `s2c`: Server-to-client messages
//! - `rpcs`: Remote procedure call messages

pub mod c2s;
pub mod s2c;
