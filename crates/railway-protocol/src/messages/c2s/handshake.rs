//! Handshake message sent by the client upon initial connection.
//!
//! Format:
//! - game_version: fixed i32 (see game_version.rs — NOT packed)
//! - name: string
//! - [if version >= V1] last_nonce: u32
//! - [if version >= V2] language: u32, chat_mode: u8
//! - [if version >= V3] platform_data: sub-message, crossplay_flags: i32
//! - [if version >= V4] unknown: u8
//! - [Reactor-modded clients only] mod list appended after the standard
//!   fields: packed_u32 count, then per mod: id string, version string,
//!   and 2 reserved/flag bytes we don't currently interpret.

use railway_hazel::MessageReader;

use crate::game_version::GameVersion;
use crate::platform_data::PlatformSpecificData;
use crate::{Language, QuickChatModes};

/// Parsed client handshake data.
#[derive(Debug)]
pub struct HandshakeData {
    pub client_version: GameVersion,
    pub name: String,
    pub language: Language,
    pub chat_mode: QuickChatModes,
    pub platform_data: Option<PlatformSpecificData>,
    /// Raw bytes left over after the standard handshake fields, if any.
    /// Some modded clients (Reactor-based, etc.) append extra data here;
    /// format not yet confirmed, so we keep it as opaque bytes rather than
    /// guessing a structure and risking another crash.
    pub trailing_mod_data: Option<bytes::Bytes>,
}

/// Deserialize a handshake from the client's hello packet.
pub fn deserialize_handshake(reader: &mut MessageReader) -> HandshakeData {
    let client_version = GameVersion::deserialize(reader);
    let name = reader.read_string();

    let version = client_version;

    let _last_nonce = if version >= GameVersion::V1 {
        reader.read_u32()
    } else {
        0
    };

    let (language, chat_mode) = if version >= GameVersion::V2 {
        (
            match reader.read_u32() {
                0 => Language::English,
                1 => Language::Spanish,
                2 => Language::Portuguese,
                3 => Language::Korean,
                4 => Language::Russian,
                5 => Language::French,
                6 => Language::German,
                7 => Language::Italian,
                8 => Language::Japanese,
                9 => Language::ChineseSimplified,
                10 => Language::ChineseTraditional,
                11 => Language::Irish,
                _ => Language::English,
            },
            match reader.read_byte() {
                0 => QuickChatModes::FreeChatOrQuickChat,
                1 => QuickChatModes::QuickChatOnly,
                _ => QuickChatModes::FreeChatOrQuickChat,
            },
        )
    } else {
        (Language::English, QuickChatModes::FreeChatOrQuickChat)
    };

    let platform_data = if version >= GameVersion::V3 {
        let platform_reader = reader.read_message();
        let pd = platform_reader.map(|mut r| PlatformSpecificData::deserialize(&mut r));
        let _crossplay_flags = reader.read_i32();
        pd
    } else {
        None
    };

    if version >= GameVersion::V4 {
        let _unknown = reader.read_byte();
    }

    // Anything left over is a non-standard extension some modded clients
    // (e.g. Reactor-based ones) append after the vanilla handshake fields.
    // We deliberately do NOT try to structurally parse it: a previous
    // attempt guessed a `count + (id, version)*` layout, but real captured
    // bytes didn't match that shape (e.g. a chunk that decoded as
    // "rotcaer" instead of a sane length-prefixed string) and parsing it
    // wrong caused a hard crash (`UnexpectedEof` while reading a bogus
    // packed-u32 length). Until the exact extension format is confirmed,
    // just record the raw trailing bytes for later analysis — never
    // interpret them.
    let trailing_mod_data = if reader.remaining() > 0 {
        Some(reader.read_bytes_to_end())
    } else {
        None
    };

    HandshakeData {
        client_version,
        name,
        language,
        chat_mode,
        platform_data,
        trailing_mod_data,
    }
}
