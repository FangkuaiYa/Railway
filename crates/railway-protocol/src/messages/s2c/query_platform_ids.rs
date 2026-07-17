//! QueryPlatformIds (flag 22) S2C message.

use railway_hazel::MessageWriter;
use crate::message_flags::MessageFlags;
use crate::platform_data::PlatformSpecificData;
use crate::GameCode;

pub fn serialize(
    writer: &mut MessageWriter,
    game_code: GameCode,
    platform_data: &[PlatformSpecificData],
) {
    writer.start_message(MessageFlags::QueryPlatformIds as u8);
    writer.write_i32(game_code);
    writer.write_packed_u32(platform_data.len() as u32);
    for pd in platform_data {
        pd.serialize(writer);
    }
    writer.end_message();
}
