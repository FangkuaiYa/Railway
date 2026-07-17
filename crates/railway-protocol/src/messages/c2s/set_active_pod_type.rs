//! SetActivePodType (flag 21) message.

use railway_hazel::MessageReader;

pub fn deserialize(reader: &mut MessageReader) -> String {
    reader.read_string()
}
