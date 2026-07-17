//! HostGame (flag 0) message: client requests to create a new game.
//!
//! Format:
//! - game_options: serialized IGameOptions (modular format)
//! - crossplay_flags: i32 (not used)
//! - game_filter_options: serialized GameFilterOptions

use railway_hazel::{MessageReader, MessageWriter};

use crate::game_options::{GameOptionsData, HideNSeekGameOptions, NormalGameOptions};
use crate::CrossplayFlags;

/// Filter options for public game listings.
///
/// Matches C# Impostor's `GameFilterOptions`: a list of filter tag strings
/// (e.g. "Beginner", "Expert", "Casual") that players can use to find games.
/// The actual max_players/map/num_impostors values come from the game options,
/// NOT from this struct.
#[derive(Debug, Clone, Default)]
pub struct GameFilterOptions {
    /// Filter tags applied to this game (e.g. "Beginner").
    pub filter_tags: Vec<String>,
}

impl GameFilterOptions {
    pub fn serialize(&self, writer: &mut MessageWriter) {
        writer.write_packed_u32(self.filter_tags.len() as u32);
        for tag in &self.filter_tags {
            writer.write_string(tag);
        }
    }

    pub fn deserialize(reader: &mut MessageReader) -> Self {
        let count = reader.read_packed_u32() as usize;
        let mut filter_tags = Vec::with_capacity(count.min(16)); // safety cap
        for _ in 0..count {
            filter_tags.push(reader.read_string());
        }
        Self { filter_tags }
    }
}

/// Deserialize game options from a HostGame message.
///
/// Game mode byte mapping (supports both vanilla and AllTheRoles mod):
///
/// | Byte | Vanilla            | AllTheRoles          |
/// |------|--------------------|----------------------|
/// | 0    | Normal             | (None — extra slot)  |
/// | 1    | HideNSeek          | Normal               |
/// | 2    | —                  | HideNSeek            |
/// | 3    | —                  | NormalFools          |
/// | 4    | —                  | SeekFools            |
///
/// When byte=1 is ambiguous (vanilla HnS vs ATR Normal), we peek at the
/// next byte to distinguish: Normal options start with max_players (u8),
/// while HnS options start with a different field layout. If we can't
/// disambiguate we fall back to Normal (the far more common case for
/// modded clients sending byte=1).
pub fn deserialize_game_options(reader: &mut MessageReader) -> Box<dyn GameOptionsData> {
    let _length = reader.read_packed_u32();
    let version = reader.read_byte();

    let options_reader = reader.read_message();
    let mut options_reader = match options_reader {
        Some(r) => r,
        None => {
            tracing::warn!("HostGame: missing game options sub-message, using defaults");
            return Box::new(NormalGameOptions::default());
        }
    };
    let game_mode = options_reader.read_byte();

    match game_mode {
        // ── Vanilla Among Us ──────────────────────────────────────
        0 => {
            // Vanilla: Normal
            let mut opts = NormalGameOptions::default();
            opts.version = version;
            opts.deserialize(&mut options_reader);
            Box::new(opts)
        }
        // ── Vanilla HnS / AllTheRoles Normal (ambiguous) ──────────
        1 => {
            if options_reader.remaining() >= 4 {
                let mut opts = NormalGameOptions::default();
                opts.version = version;
                opts.deserialize(&mut options_reader);
                Box::new(opts)
            } else {
                let mut opts = HideNSeekGameOptions::default();
                opts.version = version;
                opts.deserialize(&mut options_reader);
                Box::new(opts)
            }
        }
        // ── Vanilla / AllTheRoles HnS ─────────────────────────────
        2 => {
            // AllTheRoles: HideNSeek
            let mut opts = HideNSeekGameOptions::default();
            opts.version = version;
            opts.deserialize(&mut options_reader);
            Box::new(opts)
        }
        // ── AllTheRoles NormalFools ───────────────────────────────
        3 => {
            // AllTheRoles: NormalFools (treated as Normal)
            let mut opts = NormalGameOptions::default();
            opts.version = version;
            opts.deserialize(&mut options_reader);
            Box::new(opts)
        }
        // ── AllTheRoles SeekFools ────────────────────────────────
        4 => {
            // AllTheRoles: SeekFools (treated as HideNSeek)
            let mut opts = HideNSeekGameOptions::default();
            opts.version = version;
            opts.deserialize(&mut options_reader);
            Box::new(opts)
        }
        // ── Unknown ──────────────────────────────────────────────
        other => {
            tracing::warn!(
                "GAME_OPTIONS: unknown game_mode_byte={}, falling back to Normal with defaults",
                other
            );
            // Still need to consume the reader's data to keep position correct,
            // but since we don't know the format, just create defaults.
            Box::new(NormalGameOptions::default())
        }
    }
}

/// Deserialize the full HostGame message.
pub fn deserialize(
    reader: &mut MessageReader,
) -> (
    Box<dyn GameOptionsData>,
    CrossplayFlags,
    GameFilterOptions,
) {
    let game_options = deserialize_game_options(reader);
    let _crossplay_flags = reader.read_i32();
    let filter_options = GameFilterOptions::deserialize(reader);

    (game_options, CrossplayFlags::None, filter_options)
}
