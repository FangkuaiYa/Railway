//! Game options serialization and types.
//!
//! Among Us supports two game modes with different option structures:
//! - Normal (includes NormalFools): Classic Among Us with roles
//! - HideNSeek (includes SeekFools): Hide and Seek mode

use railway_hazel::MessageReader;
use railway_hazel::MessageWriter;
use crate::GameModes;

/// Trait for game option types.
pub trait GameOptionsData: Send + Sync {
    /// The game mode these options are for.
    fn game_mode(&self) -> GameModes;

    /// The protocol version of these options.
    fn version(&self) -> u8;

    /// Serialize options to a message writer.
    fn serialize(&self, writer: &mut MessageWriter);

    /// Deserialize options from a message reader.
    fn deserialize(&mut self, reader: &mut MessageReader);

    /// Return self as `&dyn Any` for downcasting.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Standard Among Us game options.
///
/// Field layout matches `NormalGameOptionsV10.Deserialize` EXACTLY (this is
/// the version the client actually sends — confirmed by the `version=10`
/// seen in HostGame logs). A previous version of this struct used an older
/// V7-era layout (no SpecialMode/RulesPreset/Keywords/Tag fields, and a
/// `language: u32` field that doesn't exist in V10 at all) — every single
/// field after the version byte was misaligned by one or more fields,
/// which is why `max_players` was reading `SpecialMode`'s byte instead
/// (almost always 0 for a normal game — exactly the "MaxPlayers shows 0"
/// symptom).
#[derive(Debug, Clone)]
pub struct NormalGameOptions {
    pub version: u8,
    pub special_mode: u8,
    pub rules_preset: u8,
    pub max_players: u8,
    /// `GameKeywords` bitmask (language selection for chat/quick-chat).
    pub keywords: u32,
    pub map: u8,
    pub player_speed_mod: f32,
    pub crewmate_vision_mod: f32,
    pub impostor_vision_mod: f32,
    pub kill_cooldown: f32,
    pub num_common_tasks: u8,
    pub num_long_tasks: u8,
    pub num_short_tasks: u8,
    pub num_emergency_meetings: u32,
    pub num_impostors: u8,
    pub kill_distance: u8,
    pub discussion_time: u32,
    pub voting_time: u32,
    pub is_default: bool,
    /// NOTE: a single byte in V10 (`(int)reader.ReadByte()`), not a u32.
    pub emergency_cooldown: u8,
    pub confirm_impostor: bool,
    pub visual_tasks: bool,
    pub anonymous_votes: bool,
    pub task_bar_mode: u8,
    pub tag: u8,
    pub role_options: RoleOptions,
}

impl Default for NormalGameOptions {
    fn default() -> Self {
        Self {
            version: 10,
            special_mode: 0,
            rules_preset: 0,
            max_players: 10,
            keywords: 0,
            map: 0,
            player_speed_mod: 1.0,
            crewmate_vision_mod: 1.0,
            impostor_vision_mod: 1.5,
            kill_cooldown: 25.0,
            num_common_tasks: 1,
            num_long_tasks: 1,
            num_short_tasks: 2,
            num_emergency_meetings: 1,
            num_impostors: 2,
            kill_distance: 1,
            discussion_time: 15,
            voting_time: 120,
            is_default: true,
            emergency_cooldown: 15,
            confirm_impostor: true,
            visual_tasks: false,
            anonymous_votes: false,
            task_bar_mode: 0,
            tag: 0,
            role_options: RoleOptions::default(),
        }
    }
}

impl GameOptionsData for NormalGameOptions {
    fn game_mode(&self) -> GameModes {
        GameModes::Normal
    }

    fn version(&self) -> u8 {
        self.version
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// Serializes/deserializes with version-aware field presence, matching
    /// what each real client version actually sends on the wire (verified
    /// against the client's own `NormalGameOptionsV07/V08/V09/V10`
    /// deserializers) — similar to how C# Impostor's `GameOptionsFactory`
    /// dispatches by version instead of assuming a single fixed format:
    /// - V7: no SpecialMode/RulesPreset, no Tag
    /// - V8: has SpecialMode/RulesPreset, still no Tag
    /// - V9+: has SpecialMode/RulesPreset AND Tag
    fn serialize(&self, writer: &mut MessageWriter) {
        if self.version >= 8 {
            writer.write_byte(self.special_mode);
            writer.write_byte(self.rules_preset);
        }
        writer.write_byte(self.max_players);
        writer.write_u32(self.keywords);
        writer.write_byte(self.map);
        writer.write_f32(self.player_speed_mod);
        writer.write_f32(self.crewmate_vision_mod);
        writer.write_f32(self.impostor_vision_mod);
        writer.write_f32(self.kill_cooldown);
        writer.write_byte(self.num_common_tasks);
        writer.write_byte(self.num_long_tasks);
        writer.write_byte(self.num_short_tasks);
        writer.write_u32(self.num_emergency_meetings);
        writer.write_byte(self.num_impostors);
        writer.write_byte(self.kill_distance);
        writer.write_u32(self.discussion_time);
        writer.write_u32(self.voting_time);
        writer.write_bool(self.is_default);
        writer.write_byte(self.emergency_cooldown);
        writer.write_bool(self.confirm_impostor);
        writer.write_bool(self.visual_tasks);
        writer.write_bool(self.anonymous_votes);
        writer.write_byte(self.task_bar_mode);
        if self.version >= 9 {
            writer.write_byte(self.tag);
        }
        self.role_options.serialize(writer);
    }

    fn deserialize(&mut self, reader: &mut MessageReader) {
        if self.version >= 8 {
            self.special_mode = reader.read_byte();
            self.rules_preset = reader.read_byte();
        }
        self.max_players = reader.read_byte();
        self.keywords = reader.read_u32();
        self.map = reader.read_byte();
        self.player_speed_mod = reader.read_f32();
        self.crewmate_vision_mod = reader.read_f32();
        self.impostor_vision_mod = reader.read_f32();
        self.kill_cooldown = reader.read_f32();
        self.num_common_tasks = reader.read_byte();
        self.num_long_tasks = reader.read_byte();
        self.num_short_tasks = reader.read_byte();
        self.num_emergency_meetings = reader.read_u32();
        self.num_impostors = reader.read_byte();
        self.kill_distance = reader.read_byte();
        self.discussion_time = reader.read_u32();
        self.voting_time = reader.read_u32();
        self.is_default = reader.read_bool();
        self.emergency_cooldown = reader.read_byte();
        self.confirm_impostor = reader.read_bool();
        self.visual_tasks = reader.read_bool();
        self.anonymous_votes = reader.read_bool();
        self.task_bar_mode = reader.read_byte();
        if self.version >= 9 {
            self.tag = reader.read_byte();
        }
        self.role_options.deserialize(reader);
    }
}

/// Hide and Seek game options.
///
/// Field layout matches `HideNSeekGameOptionsV10.Deserialize` exactly.
/// Note: unlike Normal mode, HnS has NO `kill_cooldown` field at all, and
/// the real struct ends at `Tag` — there's no trailing role-options block
/// in the wire format for this mode.
#[derive(Debug, Clone)]
pub struct HideNSeekGameOptions {
    pub version: u8,
    pub special_mode: u8,
    pub rules_preset: u8,
    pub max_players: u8,
    pub keywords: u32,
    pub map: u8,
    pub player_speed_mod: f32,
    pub crewmate_vision_mod: f32,
    pub impostor_vision_mod: f32,
    pub num_common_tasks: u8,
    pub num_long_tasks: u8,
    pub num_short_tasks: u8,
    pub is_default: bool,
    pub crewmate_vent_uses: i32,
    pub escape_time: f32,
    pub crewmate_flashlight_size: f32,
    pub impostor_flashlight_size: f32,
    pub use_flashlight: bool,
    pub seeker_final_map: bool,
    pub final_countdown_time: f32,
    pub seeker_final_speed: f32,
    pub seeker_pings: bool,
    pub show_crewmate_names: bool,
    pub impostor_player_id: i32,
    pub max_ping_time: f32,
    pub crewmate_time_in_vent: f32,
    pub tag: u8,
}

impl Default for HideNSeekGameOptions {
    fn default() -> Self {
        Self {
            version: 10,
            special_mode: 0,
            rules_preset: 0,
            max_players: 10,
            keywords: 0,
            map: 0,
            player_speed_mod: 1.0,
            crewmate_vision_mod: 1.0,
            impostor_vision_mod: 1.5,
            num_common_tasks: 1,
            num_long_tasks: 1,
            num_short_tasks: 2,
            is_default: true,
            crewmate_vent_uses: 0,
            escape_time: 6.0,
            crewmate_flashlight_size: 1.0,
            impostor_flashlight_size: 0.5,
            use_flashlight: true,
            seeker_final_map: true,
            final_countdown_time: 10.0,
            seeker_final_speed: 1.5,
            seeker_pings: true,
            show_crewmate_names: true,
            impostor_player_id: -1,
            max_ping_time: 4.0,
            crewmate_time_in_vent: 0.0,
            tag: 0,
        }
    }
}

impl GameOptionsData for HideNSeekGameOptions {
    fn game_mode(&self) -> GameModes {
        GameModes::HideNSeek
    }

    fn version(&self) -> u8 {
        self.version
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn serialize(&self, writer: &mut MessageWriter) {
        if self.version >= 8 {
            writer.write_byte(self.special_mode);
            writer.write_byte(self.rules_preset);
        }
        writer.write_byte(self.max_players);
        writer.write_u32(self.keywords);
        writer.write_byte(self.map);
        writer.write_f32(self.player_speed_mod);
        writer.write_f32(self.crewmate_vision_mod);
        writer.write_f32(self.impostor_vision_mod);
        writer.write_byte(self.num_common_tasks);
        writer.write_byte(self.num_long_tasks);
        writer.write_byte(self.num_short_tasks);
        writer.write_bool(self.is_default);
        writer.write_i32(self.crewmate_vent_uses);
        writer.write_f32(self.escape_time);
        writer.write_f32(self.crewmate_flashlight_size);
        writer.write_f32(self.impostor_flashlight_size);
        writer.write_bool(self.use_flashlight);
        writer.write_bool(self.seeker_final_map);
        writer.write_f32(self.final_countdown_time);
        writer.write_f32(self.seeker_final_speed);
        writer.write_bool(self.seeker_pings);
        writer.write_bool(self.show_crewmate_names);
        writer.write_i32(self.impostor_player_id);
        writer.write_f32(self.max_ping_time);
        writer.write_f32(self.crewmate_time_in_vent);
        if self.version >= 9 {
            writer.write_byte(self.tag);
        }
    }

    fn deserialize(&mut self, reader: &mut MessageReader) {
        if self.version >= 8 {
            self.special_mode = reader.read_byte();
            self.rules_preset = reader.read_byte();
        }
        self.max_players = reader.read_byte();
        self.keywords = reader.read_u32();
        self.map = reader.read_byte();
        self.player_speed_mod = reader.read_f32();
        self.crewmate_vision_mod = reader.read_f32();
        self.impostor_vision_mod = reader.read_f32();
        self.num_common_tasks = reader.read_byte();
        self.num_long_tasks = reader.read_byte();
        self.num_short_tasks = reader.read_byte();
        self.is_default = reader.read_bool();
        self.crewmate_vent_uses = reader.read_i32();
        self.escape_time = reader.read_f32();
        self.crewmate_flashlight_size = reader.read_f32();
        self.impostor_flashlight_size = reader.read_f32();
        self.use_flashlight = reader.read_bool();
        self.seeker_final_map = reader.read_bool();
        self.final_countdown_time = reader.read_f32();
        self.seeker_final_speed = reader.read_f32();
        self.seeker_pings = reader.read_bool();
        self.show_crewmate_names = reader.read_bool();
        self.impostor_player_id = reader.read_i32();
        self.max_ping_time = reader.read_f32();
        self.crewmate_time_in_vent = reader.read_f32();
        if self.version >= 9 {
            self.tag = reader.read_byte();
        }
    }
}

/// Role rate: how many players can get this role and the percentage chance.
#[derive(Debug, Clone, Copy)]
pub struct RoleRate {
    /// Maximum number of players that can be assigned this role.
    pub max_count: u8,
    /// Percentage chance (0–100) of being assigned this role.
    pub chance: u8,
}

impl RoleRate {
    fn serialize(&self, writer: &mut MessageWriter) {
        writer.write_byte(self.max_count);
        writer.write_byte(self.chance);
    }

    fn deserialize(reader: &mut MessageReader) -> Self {
        Self {
            max_count: reader.read_byte(),
            chance: reader.read_byte(),
        }
    }
}

impl Default for RoleRate {
    fn default() -> Self {
        Self { max_count: 1, chance: 0 }
    }
}

/// Role-specific options for Normal and HideNSeek modes.
///
/// Wire format matches C# `RoleOptionsCollection` exactly:
/// - Role count as packed u32
/// - For each role: u16 role type → RoleRate (2 bytes) → sub-message with role-specific data
#[derive(Debug, Clone)]
pub struct RoleOptions {
    pub scientist: IndividualRoleOptions<ScientistRoleData>,
    pub engineer: IndividualRoleOptions<EngineerRoleData>,
    pub guardian_angel: IndividualRoleOptions<GuardianAngelRoleData>,
    pub shapeshifter: IndividualRoleOptions<ShapeshifterRoleData>,
    pub noisemaker: IndividualRoleOptions<NoisemakerRoleData>,
    pub phantom: IndividualRoleOptions<PhantomRoleData>,
    pub tracker: IndividualRoleOptions<TrackerRoleData>,
    pub detective: IndividualRoleOptions<DetectiveRoleData>,
    pub viper: IndividualRoleOptions<ViperRoleData>,
}

/// Generic wrapper for a single role's rate + specific options.
#[derive(Debug, Clone)]
pub struct IndividualRoleOptions<T: RoleDataTrait + Default> {
    pub rate: RoleRate,
    pub data: T,
}

impl<T: RoleDataTrait + Default> Default for IndividualRoleOptions<T> {
    fn default() -> Self {
        Self {
            rate: RoleRate::default(),
            data: T::default(),
        }
    }
}

impl<T: RoleDataTrait + Default> IndividualRoleOptions<T> {
    fn deserialize_sub_message(&mut self, reader: &mut MessageReader) {
        self.data.deserialize(reader);
    }
}

/// Trait for per-role option data that can serialize/deserialize at the byte level.
pub trait RoleDataTrait {
    fn role_type() -> u16;
    fn serialize(&self, writer: &mut MessageWriter);
    fn deserialize(&mut self, reader: &mut MessageReader);
}

impl Default for RoleOptions {
    fn default() -> Self {
        Self {
            scientist: IndividualRoleOptions::default(),
            engineer: IndividualRoleOptions::default(),
            guardian_angel: IndividualRoleOptions::default(),
            shapeshifter: IndividualRoleOptions::default(),
            noisemaker: IndividualRoleOptions::default(),
            phantom: IndividualRoleOptions::default(),
            tracker: IndividualRoleOptions::default(),
            detective: IndividualRoleOptions::default(),
            viper: IndividualRoleOptions::default(),
        }
    }
}

impl RoleOptions {
    pub fn serialize(&self, writer: &mut MessageWriter) {
        // Count of known roles (always 9)
        writer.write_packed_u32(9);

        self.scientist.serialize_entry(writer);
        self.engineer.serialize_entry(writer);
        self.guardian_angel.serialize_entry(writer);
        self.shapeshifter.serialize_entry(writer);
        self.noisemaker.serialize_entry(writer);
        self.phantom.serialize_entry(writer);
        self.tracker.serialize_entry(writer);
        self.detective.serialize_entry(writer);
        self.viper.serialize_entry(writer);
    }

    pub fn deserialize(&mut self, reader: &mut MessageReader) {
        let role_count = reader.read_packed_u32();
        for _ in 0..role_count {
            let role_type = reader.read_u16();
            let rate = RoleRate::deserialize(reader);
            let mut sub_reader = match reader.read_message() {
                Some(r) => r,
                None => continue,
            };
            match role_type {
                t if t == ScientistRoleData::role_type() => {
                    self.scientist.rate = rate;
                    self.scientist.deserialize_sub_message(&mut sub_reader);
                }
                t if t == EngineerRoleData::role_type() => {
                    self.engineer.rate = rate;
                    self.engineer.deserialize_sub_message(&mut sub_reader);
                }
                t if t == GuardianAngelRoleData::role_type() => {
                    self.guardian_angel.rate = rate;
                    self.guardian_angel.deserialize_sub_message(&mut sub_reader);
                }
                t if t == ShapeshifterRoleData::role_type() => {
                    self.shapeshifter.rate = rate;
                    self.shapeshifter.deserialize_sub_message(&mut sub_reader);
                }
                t if t == NoisemakerRoleData::role_type() => {
                    self.noisemaker.rate = rate;
                    self.noisemaker.deserialize_sub_message(&mut sub_reader);
                }
                t if t == PhantomRoleData::role_type() => {
                    self.phantom.rate = rate;
                    self.phantom.deserialize_sub_message(&mut sub_reader);
                }
                t if t == TrackerRoleData::role_type() => {
                    self.tracker.rate = rate;
                    self.tracker.deserialize_sub_message(&mut sub_reader);
                }
                t if t == DetectiveRoleData::role_type() => {
                    self.detective.rate = rate;
                    self.detective.deserialize_sub_message(&mut sub_reader);
                }
                t if t == ViperRoleData::role_type() => {
                    self.viper.rate = rate;
                    self.viper.deserialize_sub_message(&mut sub_reader);
                }
                _ => {} // unknown role, skip
            }
        }
    }
}

impl<T: RoleDataTrait + Default> IndividualRoleOptions<T> {
    fn serialize_entry(&self, writer: &mut MessageWriter) {
        // u16 role type (fixed 2 bytes, matching C# writer.Write((ushort)key))
        writer.write_u16(T::role_type());
        // RoleRate (2 bytes: max_count, chance)
        self.rate.serialize(writer);
        // Sub-message wrapper (C# StartMessage/EndMessage)
        writer.start_message(0);
        self.data.serialize(writer);
        writer.end_message();
    }
}

// ─── Per-role data types ───────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ScientistRoleData {
    pub cooldown: u8,
    pub battery_charge: u8,
}
impl Default for ScientistRoleData { fn default() -> Self { Self { cooldown: 15, battery_charge: 5 } } }
impl RoleDataTrait for ScientistRoleData {
    fn role_type() -> u16 { 2 }
    fn serialize(&self, w: &mut MessageWriter) { w.write_byte(self.cooldown); w.write_byte(self.battery_charge); }
    fn deserialize(&mut self, r: &mut MessageReader) { self.cooldown = r.read_byte(); self.battery_charge = r.read_byte(); }
}

#[derive(Debug, Clone)]
pub struct EngineerRoleData {
    pub cooldown: u8,
    pub in_vent_max_time: u8,
}
impl Default for EngineerRoleData { fn default() -> Self { Self { cooldown: 30, in_vent_max_time: 15 } } }
impl RoleDataTrait for EngineerRoleData {
    fn role_type() -> u16 { 3 }
    fn serialize(&self, w: &mut MessageWriter) { w.write_byte(self.cooldown); w.write_byte(self.in_vent_max_time); }
    fn deserialize(&mut self, r: &mut MessageReader) { self.cooldown = r.read_byte(); self.in_vent_max_time = r.read_byte(); }
}

#[derive(Debug, Clone)]
pub struct GuardianAngelRoleData {
    pub cooldown: u8,
    pub protection_duration: u8,
    pub impostors_can_see: bool,
}
impl Default for GuardianAngelRoleData { fn default() -> Self { Self { cooldown: 60, protection_duration: 10, impostors_can_see: false } } }
impl RoleDataTrait for GuardianAngelRoleData {
    fn role_type() -> u16 { 4 }
    fn serialize(&self, w: &mut MessageWriter) { w.write_byte(self.cooldown); w.write_byte(self.protection_duration); w.write_bool(self.impostors_can_see); }
    fn deserialize(&mut self, r: &mut MessageReader) { self.cooldown = r.read_byte(); self.protection_duration = r.read_byte(); self.impostors_can_see = r.read_bool(); }
}

#[derive(Debug, Clone)]
pub struct ShapeshifterRoleData {
    pub leave_skin: bool,
    pub cooldown: u8,
    pub duration: u8,
}
impl Default for ShapeshifterRoleData { fn default() -> Self { Self { leave_skin: false, cooldown: 10, duration: 30 } } }
impl RoleDataTrait for ShapeshifterRoleData {
    fn role_type() -> u16 { 5 }
    fn serialize(&self, w: &mut MessageWriter) { w.write_bool(self.leave_skin); w.write_byte(self.cooldown); w.write_byte(self.duration); }
    fn deserialize(&mut self, r: &mut MessageReader) { self.leave_skin = r.read_bool(); self.cooldown = r.read_byte(); self.duration = r.read_byte(); }
}

#[derive(Debug, Clone)]
pub struct NoisemakerRoleData {
    pub alert_duration: u8,
    pub impostor_alert: bool,
}
impl Default for NoisemakerRoleData { fn default() -> Self { Self { alert_duration: 10, impostor_alert: true } } }
impl RoleDataTrait for NoisemakerRoleData {
    fn role_type() -> u16 { 8 }
    fn serialize(&self, w: &mut MessageWriter) { w.write_byte(self.alert_duration); w.write_bool(self.impostor_alert); }
    fn deserialize(&mut self, r: &mut MessageReader) { self.alert_duration = r.read_byte(); self.impostor_alert = r.read_bool(); }
}

#[derive(Debug, Clone)]
pub struct PhantomRoleData {
    pub cooldown: u8,
    pub duration: u8,
}
impl Default for PhantomRoleData { fn default() -> Self { Self { cooldown: 15, duration: 30 } } }
impl RoleDataTrait for PhantomRoleData {
    fn role_type() -> u16 { 9 }
    fn serialize(&self, w: &mut MessageWriter) { w.write_byte(self.cooldown); w.write_byte(self.duration); }
    fn deserialize(&mut self, r: &mut MessageReader) { self.cooldown = r.read_byte(); self.duration = r.read_byte(); }
}

#[derive(Debug, Clone)]
pub struct TrackerRoleData {
    pub cooldown: u8,
    pub duration: u8,
    pub delay: u8,
}
impl Default for TrackerRoleData { fn default() -> Self { Self { cooldown: 15, duration: 30, delay: 1 } } }
impl RoleDataTrait for TrackerRoleData {
    fn role_type() -> u16 { 10 }
    fn serialize(&self, w: &mut MessageWriter) { w.write_byte(self.cooldown); w.write_byte(self.duration); w.write_byte(self.delay); }
    fn deserialize(&mut self, r: &mut MessageReader) { self.cooldown = r.read_byte(); self.duration = r.read_byte(); self.delay = r.read_byte(); }
}

#[derive(Debug, Clone)]
pub struct DetectiveRoleData {
    pub suspect_limit: u8,
}
impl Default for DetectiveRoleData { fn default() -> Self { Self { suspect_limit: 3 } } }
impl RoleDataTrait for DetectiveRoleData {
    fn role_type() -> u16 { 12 }
    fn serialize(&self, w: &mut MessageWriter) { w.write_byte(self.suspect_limit); }
    fn deserialize(&mut self, r: &mut MessageReader) { self.suspect_limit = r.read_byte(); }
}

/// Viper role: dissolve time.
#[derive(Debug, Clone)]
pub struct ViperRoleData {
    pub dissolve_time: u8,
}
impl Default for ViperRoleData { fn default() -> Self { Self { dissolve_time: 15 } } }
impl RoleDataTrait for ViperRoleData {
    fn role_type() -> u16 { 18 }
    fn serialize(&self, w: &mut MessageWriter) { w.write_byte(self.dissolve_time); }
    fn deserialize(&mut self, r: &mut MessageReader) { self.dissolve_time = r.read_byte(); }
}
