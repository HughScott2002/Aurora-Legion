//! Settings persistence. The daemon is the only process that touches the
//! settings file; GUI and CLI go through IPC.
//!
//! Location: `$XDG_CONFIG_HOME/aurora/settings.json`.
//!
//! Two migrations can happen on load. The older one moves a file from the
//! locations the pre-rename app used. The newer one converts a v1 file, in
//! which a profile *was* one lighting configuration and per-slot lighting
//! lived in a separate `hardware_slot_profiles` list, into the v2 shape
//! where a profile owns one [`Lighting`] per slot.
//!
//! The rule that governs everything here: never overwrite a settings file
//! we could not understand. A parse failure makes the settings read-only
//! for the life of the process, because the alternative is that a daemon
//! bug erases the only copy of the user's profiles on the next shutdown.

use std::{
    fs,
    path::{Path, PathBuf},
};

use aurora_protocol::{
    custom_effect::CustomEffect,
    effects::{Brightness, Direction, Effects},
    ipc::{SlotSelection, MAX_SAVED_CUSTOM_EFFECTS, MAX_SAVED_PROFILES},
    profile::{Lighting, Profile, Zones, SLOT_COUNT},
};
use serde::{Deserialize, Serialize};

pub const CONFIG_DIR_NAME: &str = "aurora";
pub const SETTINGS_FILE_NAME: &str = "settings.json";

/// Config dir used before the project was renamed to aurora; still read
/// (never written) during migration.
const PRE_RENAME_CONFIG_DIR_NAME: &str = "legion-kb-rgb";

/// How many numbered backups to try before giving up on securing one. A
/// migration that cannot back the original up does not run at all.
const MAX_BACKUP_ATTEMPTS: u32 = 16;

#[derive(Debug, Deserialize, Serialize)]
pub struct Settings {
    pub profiles: Vec<Profile>,
    /// Custom effects. Keeps its historical field name.
    pub effects: Vec<CustomEffect>,
    pub current_profile: Profile,
    /// Which Fn+Space position was live when the daemon last ran.
    pub active_slot: SlotSelection,

    /// Set when the file on disk could not be parsed. While this holds a
    /// reason, [`Settings::save`] refuses to write, so an unreadable file
    /// is never overwritten by defaults.
    #[serde(skip)]
    save_blocked: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            effects: Vec::new(),
            current_profile: Profile::default(),
            active_slot: SlotSelection::First,
            save_blocked: None,
        }
    }
}

pub fn settings_file_path() -> Option<PathBuf> {
    let config_dir = dirs::config_dir()?;

    let mut path = config_dir;
    path.push(CONFIG_DIR_NAME);
    path.push(SETTINGS_FILE_NAME);
    Some(path)
}

impl Settings {
    /// Load settings, converting older shapes on the way. Never fails: a
    /// daemon that cannot read its settings still lights the keyboard.
    pub fn load_or_migrate() -> Self {
        let Some(path) = settings_file_path() else {
            eprintln!("settings: no config directory available, starting with defaults");
            return Self {
                save_blocked: Some("no config directory available".to_string()),
                ..Self::default()
            };
        };

        if path.is_file() {
            return Self::load_from(&path);
        }

        let Some(legacy_path) = find_legacy_settings_file() else {
            eprintln!(
                "settings: no settings file found, starting fresh at {}",
                path.display()
            );
            return Self::default();
        };

        // Migrating from a different path leaves the original untouched, so
        // no backup is needed here.
        eprintln!(
            "settings: migrating settings from {} to {}",
            legacy_path.display(),
            path.display()
        );
        let mut settings = Self::load_from(&legacy_path);
        settings.save_blocked = None;
        let save_result = settings.save_to(&path);
        if let Err(message) = save_result {
            eprintln!("settings: could not write the migrated file: {message}");
        }
        settings
    }

    fn load_from(path: &Path) -> Self {
        let read_result = fs::read_to_string(path);

        let contents = match read_result {
            Ok(contents) => contents,
            Err(error) => {
                let reason = format!("could not read {}: {error}", path.display());
                eprintln!("settings: {reason}, using defaults");
                return Self {
                    save_blocked: Some(reason),
                    ..Self::default()
                };
            }
        };

        // Current shape first: the common case after the first upgrade.
        let current_result: Result<Settings, serde_json::Error> = serde_json::from_str(&contents);
        if let Ok(settings) = current_result {
            return settings.with_reported_bounds();
        }

        // v1: a flat profile plus a separate per-slot list.
        let legacy_result: Result<LegacySettings, serde_json::Error> =
            serde_json::from_str(&contents);
        let legacy = match legacy_result {
            Ok(legacy) => legacy,
            Err(error) => {
                let reason = format!("could not parse {}: {error}", path.display());
                eprintln!("settings: {reason}");
                preserve_corrupt_file(path);
                eprintln!(
                    "settings: refusing to overwrite it; this session will not save settings"
                );
                return Self {
                    save_blocked: Some(reason),
                    ..Self::default()
                };
            }
        };

        eprintln!("settings: converting v1 settings at {}", path.display());

        let backup_result = secure_backup(path);
        match backup_result {
            Ok(backup_path) => {
                eprintln!("settings: kept the v1 file at {}", backup_path.display());
            }
            Err(message) => {
                // Without a backup, writing v2 over the v1 file is the one
                // irreversible step in this whole path. Do not take it.
                let reason = format!("could not back up the v1 settings file: {message}");
                eprintln!("settings: {reason}");
                eprintln!(
                    "settings: converting in memory only; this session will not save settings"
                );
                let mut settings = legacy.into_settings();
                settings.save_blocked = Some(reason);
                return settings;
            }
        }

        legacy.into_settings().with_reported_bounds()
    }

    /// Log when a loaded file is over the bounds new additions are held to.
    /// Existing entries are kept: refusing to load them, or silently
    /// dropping them, both lose user data. The add paths enforce the bound
    /// so the file cannot grow further.
    fn with_reported_bounds(self) -> Self {
        if self.profiles.len() > MAX_SAVED_PROFILES {
            eprintln!(
                "settings: {} saved profiles exceeds the {MAX_SAVED_PROFILES} limit; keeping them, but no more can be added",
                self.profiles.len()
            );
        }
        if self.effects.len() > MAX_SAVED_CUSTOM_EFFECTS {
            eprintln!(
                "settings: {} saved custom effects exceeds the {MAX_SAVED_CUSTOM_EFFECTS} limit; keeping them, but no more can be added",
                self.effects.len()
            );
        }
        self
    }

    /// Save to the XDG path. The error is returned rather than logged and
    /// swallowed: the core keeps its dirty flag set on failure and reports
    /// the reason through daemon state.
    pub fn save(&self) -> Result<(), String> {
        if let Some(reason) = &self.save_blocked {
            return Err(format!("settings are read-only this session: {reason}"));
        }

        let Some(path) = settings_file_path() else {
            return Err("no config directory available".to_string());
        };

        self.save_to(&path)
    }

    fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            let create_result = fs::create_dir_all(parent);
            if let Err(error) = create_result {
                return Err(format!("could not create {}: {error}", parent.display()));
            }
        }

        let serialized = match serde_json::to_string_pretty(self) {
            Ok(serialized) => serialized,
            Err(error) => return Err(format!("could not serialize settings: {error}")),
        };

        // Write to a sibling temp file and rename so a crash mid-write can
        // never leave a half-written settings file behind.
        let temp_path = path.with_extension("json.tmp");
        let write_result = fs::write(&temp_path, serialized);
        if let Err(error) = write_result {
            return Err(format!("could not write {}: {error}", temp_path.display()));
        }

        let rename_result = fs::rename(&temp_path, path);
        if let Err(error) = rename_result {
            return Err(format!(
                "could not move {} into place: {error}",
                temp_path.display()
            ));
        }

        Ok(())
    }
}

/// Copy `path` to a backup that does not already exist. Returns the backup
/// path. Numbered because a previous interrupted migration may have left
/// one behind, and that copy must not be clobbered.
fn secure_backup(path: &Path) -> Result<PathBuf, String> {
    let mut attempt: u32 = 0;

    while attempt < MAX_BACKUP_ATTEMPTS {
        let candidate = if attempt == 0 {
            path.with_extension("json.v1-backup")
        } else {
            path.with_extension(format!("json.v1-backup.{attempt}"))
        };

        if candidate.exists() {
            attempt += 1;
            continue;
        }

        let copy_result = fs::copy(path, &candidate);
        match copy_result {
            Ok(_) => return Ok(candidate),
            Err(error) => return Err(format!("{}: {error}", candidate.display())),
        }
    }

    Err(format!("{MAX_BACKUP_ATTEMPTS} backup names already taken"))
}

/// Keep a copy of an unparseable settings file so a daemon bug or a manual
/// edit gone wrong can be recovered from instead of silently overwritten.
fn preserve_corrupt_file(path: &Path) {
    let backup_path = path.with_extension("json.invalid");
    let copy_result = fs::copy(path, &backup_path);
    match copy_result {
        Ok(_) => eprintln!(
            "settings: kept the unparseable file at {}",
            backup_path.display()
        ),
        Err(error) => eprintln!("settings: could not back up the unparseable file: {error}"),
    }
}

// --- v1 shapes -----------------------------------------------------------

/// A v1 profile: one flat lighting configuration with a name. The v2
/// `Settings` cannot deserialize these at all, so they get their own types
/// and an explicit conversion rather than a serde compatibility shim.
#[derive(Deserialize)]
struct LegacyProfile {
    #[serde(default)]
    name: Option<String>,
    rgb_zones: Zones,
    effect: Effects,
    direction: Direction,
    speed: u8,
    brightness: Brightness,
}

impl LegacyProfile {
    fn into_lighting(self) -> Lighting {
        Lighting {
            rgb_zones: self.rgb_zones,
            effect: self.effect,
            direction: self.direction,
            speed: self.speed,
            brightness: self.brightness,
        }
    }
}

#[derive(Deserialize)]
struct LegacySettings {
    #[serde(default)]
    profiles: Vec<LegacyProfile>,
    #[serde(default)]
    effects: Vec<CustomEffect>,
    #[serde(alias = "ui_state")]
    current_profile: LegacyProfile,
    /// v1's per-slot lighting, index 0 being slot 1. Absent on files that
    /// predate slot support.
    #[serde(default)]
    hardware_slot_profiles: Vec<LegacyProfile>,
}

impl LegacySettings {
    fn into_settings(self) -> Settings {
        let live_name = self.current_profile.name.clone();
        let live_lighting = self.current_profile.into_lighting();

        // The v1 per-slot list becomes the live profile's slots. Missing or
        // short lists pad from the live lighting, so nothing the user could
        // see is invented or lost.
        let mut slot_lightings: Vec<Lighting> = Vec::with_capacity(SLOT_COUNT);
        for legacy_slot in self.hardware_slot_profiles {
            if slot_lightings.len() == SLOT_COUNT {
                break;
            }
            slot_lightings.push(legacy_slot.into_lighting());
        }
        while slot_lightings.len() < SLOT_COUNT {
            slot_lightings.push(live_lighting.clone());
        }

        let mut slots: [Lighting; SLOT_COUNT] = Default::default();
        for (slot_index, slot) in slots.iter_mut().enumerate() {
            *slot = slot_lightings[slot_index].clone();
        }

        let current_profile = Profile {
            name: live_name,
            slots,
        };

        // v1 never recorded which slot was live. The live lighting usually
        // matches exactly one slot, which recovers it; otherwise start at
        // the first slot.
        let active_slot = match current_profile.slot_showing(&live_lighting) {
            Some(0) => SlotSelection::First,
            Some(1) => SlotSelection::Second,
            Some(2) => SlotSelection::Third,
            _ => SlotSelection::First,
        };

        // A v1 saved profile had one look. It becomes a profile whose three
        // slots match, so activating it behaves as it did before slots.
        let mut profiles: Vec<Profile> = Vec::with_capacity(self.profiles.len());
        for legacy_profile in self.profiles {
            let name = legacy_profile.name.clone();
            let lighting = legacy_profile.into_lighting();
            profiles.push(Profile::from_uniform_lighting(name, lighting));
        }

        Settings {
            profiles,
            effects: self.effects,
            current_profile,
            active_slot,
            save_blocked: None,
        }
    }
}

/// Legacy locations, most specific first: the pre-rename XDG dir, the old
/// app's `$LEGION_KEYBOARD_CONFIG` override, the old app's CWD file.
fn find_legacy_settings_file() -> Option<PathBuf> {
    if let Some(config_dir) = dirs::config_dir() {
        let mut pre_rename_path = config_dir;
        pre_rename_path.push(PRE_RENAME_CONFIG_DIR_NAME);
        pre_rename_path.push(SETTINGS_FILE_NAME);
        if pre_rename_path.is_file() {
            return Some(pre_rename_path);
        }
    }

    if let Ok(env_path) = std::env::var("LEGION_KEYBOARD_CONFIG") {
        let path = PathBuf::from(env_path);
        if path.is_file() {
            return Some(path);
        }
    }

    let cwd_path = PathBuf::from("./settings.json");
    if cwd_path.is_file() {
        return Some(cwd_path);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A v1 file exactly as 0.23 wrote it, with distinct per-slot colors
    /// and a live profile matching slot 2.
    fn v1_json_with_slots() -> String {
        r#"{
            "profiles": [{
                "name": "gaming",
                "rgb_zones": [
                    {"rgb": [10, 20, 30], "enabled": true},
                    {"rgb": [10, 20, 30], "enabled": true},
                    {"rgb": [10, 20, 30], "enabled": true},
                    {"rgb": [10, 20, 30], "enabled": true}
                ],
                "effect": "Breath", "direction": "Right", "speed": 3, "brightness": "High"
            }],
            "effects": [],
            "current_profile": {
                "name": "live",
                "rgb_zones": [
                    {"rgb": [0, 255, 0], "enabled": true},
                    {"rgb": [0, 255, 0], "enabled": true},
                    {"rgb": [0, 255, 0], "enabled": true},
                    {"rgb": [0, 255, 0], "enabled": true}
                ],
                "effect": "Static", "direction": "Left", "speed": 1, "brightness": "Low"
            },
            "hardware_slot_profiles": [
                {"name": null, "rgb_zones": [
                    {"rgb": [255, 0, 0], "enabled": true}, {"rgb": [255, 0, 0], "enabled": true},
                    {"rgb": [255, 0, 0], "enabled": true}, {"rgb": [255, 0, 0], "enabled": true}],
                 "effect": "Static", "direction": "Left", "speed": 1, "brightness": "Low"},
                {"name": null, "rgb_zones": [
                    {"rgb": [0, 255, 0], "enabled": true}, {"rgb": [0, 255, 0], "enabled": true},
                    {"rgb": [0, 255, 0], "enabled": true}, {"rgb": [0, 255, 0], "enabled": true}],
                 "effect": "Static", "direction": "Left", "speed": 1, "brightness": "Low"},
                {"name": null, "rgb_zones": [
                    {"rgb": [0, 0, 255], "enabled": true}, {"rgb": [0, 0, 255], "enabled": true},
                    {"rgb": [0, 0, 255], "enabled": true}, {"rgb": [0, 0, 255], "enabled": true}],
                 "effect": "Static", "direction": "Left", "speed": 1, "brightness": "Low"}
            ]
        }"#
        .to_string()
    }

    fn parse_v1(json: &str) -> Settings {
        let legacy: LegacySettings = serde_json::from_str(json).expect("v1 fixture should parse");
        legacy.into_settings()
    }

    #[test]
    fn v1_slot_lighting_becomes_the_live_profile_slots() {
        let settings = parse_v1(&v1_json_with_slots());

        assert_eq!(
            settings.current_profile.slots[0].rgb_array()[..3],
            [255, 0, 0]
        );
        assert_eq!(
            settings.current_profile.slots[1].rgb_array()[..3],
            [0, 255, 0]
        );
        assert_eq!(
            settings.current_profile.slots[2].rgb_array()[..3],
            [0, 0, 255]
        );
        assert_eq!(settings.current_profile.name.as_deref(), Some("live"));
    }

    /// v1 recorded no active slot. The live lighting matched slot 2 here, so
    /// the keyboard keeps showing what it was showing.
    #[test]
    fn v1_active_slot_is_recovered_from_the_live_lighting() {
        let settings = parse_v1(&v1_json_with_slots());
        assert_eq!(settings.active_slot, SlotSelection::Second);
    }

    #[test]
    fn v1_saved_profiles_become_uniform_across_slots() {
        let settings = parse_v1(&v1_json_with_slots());

        assert_eq!(settings.profiles.len(), 1);
        let gaming = &settings.profiles[0];
        assert_eq!(gaming.name.as_deref(), Some("gaming"));
        for slot in &gaming.slots {
            assert_eq!(slot.effect, Effects::Breath);
            assert_eq!(slot.speed, 3);
            assert_eq!(slot.rgb_array()[..3], [10, 20, 30]);
        }
    }

    /// Files from before slot support have no `hardware_slot_profiles`.
    /// Every slot takes the live lighting, so the keyboard looks the same
    /// on all three instead of jumping to invented colors.
    #[test]
    fn v1_without_slot_profiles_pads_from_the_live_lighting() {
        let json = r#"{
            "profiles": [], "effects": [],
            "current_profile": {
                "name": null,
                "rgb_zones": [
                    {"rgb": [7, 7, 7], "enabled": true}, {"rgb": [7, 7, 7], "enabled": true},
                    {"rgb": [7, 7, 7], "enabled": true}, {"rgb": [7, 7, 7], "enabled": true}],
                "effect": "Static", "direction": "Left", "speed": 1, "brightness": "Low"
            }
        }"#;

        let settings = parse_v1(json);
        for slot in &settings.current_profile.slots {
            assert_eq!(slot.rgb_array()[..3], [7, 7, 7]);
        }
        // Every slot matches, so no slot is identifiable; start at the first.
        assert_eq!(settings.active_slot, SlotSelection::First);
    }

    /// A short slot list must not leave an uninitialized slot behind.
    #[test]
    fn v1_with_a_short_slot_list_pads_the_rest() {
        let json = r#"{
            "profiles": [], "effects": [],
            "current_profile": {
                "name": null,
                "rgb_zones": [
                    {"rgb": [1, 1, 1], "enabled": true}, {"rgb": [1, 1, 1], "enabled": true},
                    {"rgb": [1, 1, 1], "enabled": true}, {"rgb": [1, 1, 1], "enabled": true}],
                "effect": "Static", "direction": "Left", "speed": 1, "brightness": "Low"
            },
            "hardware_slot_profiles": [
                {"name": null, "rgb_zones": [
                    {"rgb": [2, 2, 2], "enabled": true}, {"rgb": [2, 2, 2], "enabled": true},
                    {"rgb": [2, 2, 2], "enabled": true}, {"rgb": [2, 2, 2], "enabled": true}],
                 "effect": "Static", "direction": "Left", "speed": 1, "brightness": "Low"}
            ]
        }"#;

        let settings = parse_v1(json);
        assert_eq!(
            settings.current_profile.slots[0].rgb_array()[..3],
            [2, 2, 2]
        );
        assert_eq!(
            settings.current_profile.slots[1].rgb_array()[..3],
            [1, 1, 1]
        );
        assert_eq!(
            settings.current_profile.slots[2].rgb_array()[..3],
            [1, 1, 1]
        );
    }

    /// More slots than the hardware has must not panic or silently shift
    /// the mapping.
    #[test]
    fn v1_with_an_oversized_slot_list_keeps_the_first_three() {
        let mut json = serde_json::from_str::<serde_json::Value>(&v1_json_with_slots()).unwrap();
        let slots = json["hardware_slot_profiles"].as_array().unwrap().clone();
        let mut extended = slots.clone();
        extended.extend(slots);
        json["hardware_slot_profiles"] = serde_json::Value::Array(extended);

        let settings = parse_v1(&json.to_string());
        assert_eq!(settings.current_profile.slots.len(), SLOT_COUNT);
        assert_eq!(
            settings.current_profile.slots[0].rgb_array()[..3],
            [255, 0, 0]
        );
        assert_eq!(
            settings.current_profile.slots[2].rgb_array()[..3],
            [0, 0, 255]
        );
    }

    /// The destructive path. An unparseable file must leave the daemon
    /// unable to save, or shutdown overwrites the only copy with defaults.
    #[test]
    fn unparseable_settings_block_saving() {
        let settings = Settings {
            save_blocked: Some("could not parse".to_string()),
            ..Default::default()
        };

        let save_result = settings.save();
        assert!(save_result.is_err(), "a blocked save must not write");
    }

    #[test]
    fn v2_settings_round_trip() {
        let settings = Settings {
            profiles: vec![Profile::default()],
            effects: Vec::new(),
            current_profile: Profile::default(),
            active_slot: SlotSelection::Third,
            save_blocked: None,
        };

        let json = serde_json::to_string(&settings).unwrap();
        let parsed: Settings = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.active_slot, SlotSelection::Third);
        assert_eq!(parsed.current_profile, settings.current_profile);
        assert!(
            parsed.save_blocked.is_none(),
            "the block flag must not travel through the file"
        );
    }

    /// A v2 file must not be re-migrated: v1 parsing is only reached when
    /// the current shape fails.
    #[test]
    fn v2_json_does_not_parse_as_v1() {
        let settings = Settings::default();
        let json = serde_json::to_string(&settings).unwrap();

        let as_legacy: Result<LegacySettings, serde_json::Error> = serde_json::from_str(&json);
        assert!(as_legacy.is_err(), "v2 must be distinguishable from v1");
    }
}
