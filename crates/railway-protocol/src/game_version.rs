//! Game version parsing and comparison.
//!
//! Among Us game versions are encoded as a single i32:
//! `year * 25000 + month * 1800 + day * 50 + revision`
//!
//! Example: version 2021.4.25 → 505 * 25000 + 4 * 1800 + 25 * 50 + 0 = 12,632,450

use railway_hazel::MessageReader;
use railway_hazel::MessageWriter;

/// Represents an Among Us game version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GameVersion {
    /// Internal raw value (year*25000 + month*1800 + day*50 + revision).
    raw: i32,
}

impl GameVersion {
    /// Create a new GameVersion from year, month, day, and revision components.
    pub const fn new(year: i32, month: i32, day: i32, revision: i32) -> Self {
        Self {
            raw: year * 25000 + month * 1800 + day * 50 + revision,
        }
    }

    /// Create from a raw i32 value.
    pub const fn from_raw(raw: i32) -> Self {
        Self { raw }
    }

    /// Returns the raw i32 encoding.
    pub fn raw(self) -> i32 {
        self.raw
    }

    /// Returns the year component.
    pub fn year(self) -> i32 {
        self.raw / 25000
    }

    /// Returns the month component.
    pub fn month(self) -> i32 {
        (self.raw % 25000) / 1800
    }

    /// Returns the day component.
    pub fn day(self) -> i32 {
        (self.raw % 1800) / 50
    }

    /// Returns the revision component.
    pub fn revision(self) -> i32 {
        self.raw % 50
    }

    /// Returns true if the "disable server authority" flag is set.
    ///
    /// This flag is encoded by adding 25 to the revision field (which would
    /// normally overflow, thus the flag).
    pub fn has_disable_server_authority(self) -> bool {
        self.revision() < 0 || self.revision() >= 25
    }

    /// Normalize the version (remove flags from revision).
    pub fn normalize(self) -> Self {
        let mut raw = self.raw;
        // If the revision has the +25 flag, normalize it
        let rev = raw % 50;
        if rev >= 25 {
            raw -= 25;
        }
        Self { raw }
    }

    /// Serialize (write) the version to a MessageWriter.
    ///
    /// IMPORTANT: real Among Us writes `GameVersion` as a plain fixed
    /// 4-byte little-endian `int` (`writer.Write((int)value)` /
    /// `reader.ReadInt32()` in C#'s `MessageWriterExtensions`/
    /// `MessageReaderExtensions`). It is NOT a packed/variable-length
    /// integer. Using packed encoding here (as a previous version of this
    /// function did) desyncs the very first field of the handshake, which
    /// then corrupts every read that follows it.
    pub fn serialize(&self, writer: &mut MessageWriter) {
        writer.write_i32(self.raw);
    }

    /// Deserialize (read) the version from a MessageReader.
    pub fn deserialize(reader: &mut MessageReader) -> Self {
        Self {
            raw: reader.read_i32(),
        }
    }

    // Well-known versions
    pub const V1: Self = Self::new(2021, 4, 25, 0);
    pub const V2: Self = Self::new(2021, 6, 30, 0);
    pub const V3: Self = Self::new(2021, 11, 9, 0);
    pub const V4: Self = Self::new(2021, 12, 14, 0);
}

impl std::fmt::Display for GameVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}.{}.{}",
            self.year(),
            self.month(),
            self.day()
        )?;
        let rev = self.revision();
        if rev != 0 {
            write!(f, ".{}", rev)?;
        }
        Ok(())
    }
}
