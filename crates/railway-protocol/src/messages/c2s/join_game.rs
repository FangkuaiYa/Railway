//! JoinGame (flag 1) message.

use railway_hazel::MessageReader;
use crate::GameCode;

pub fn deserialize(reader: &mut MessageReader) -> GameCode {
    let code = reader.read_i32();
    let _no_crossplay = reader.read_bool();
    code
}
