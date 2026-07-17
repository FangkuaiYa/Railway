//! QueryPlatformIds (flag 22) message.

use railway_hazel::MessageReader;
use crate::GameCode;

pub fn deserialize(reader: &mut MessageReader) -> GameCode {
    reader.read_i32()
}
