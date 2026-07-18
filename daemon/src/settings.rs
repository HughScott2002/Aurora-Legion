//! Settings persistence. The daemon is the only process that touches the
//! settings file; GUI and CLI go through IPC.
//!
//! Location: `$XDG_CONFIG_HOME/aurora/settings.json`. On first run,
//! settings are migrated from the legacy locations the old app used
//! (`$LEGION_KEYBOARD_CONFIG`, then `./settings.json` in whatever directory
//! the app happened to start in).

use std::{
    fs,
    path::{Path, PathBuf},
};

use aurora_protocol::{custom_effect::CustomEffect, profile::Profile};
use serde::{Deserialize, Serialize};

pub const CONFIG_DIR_NAME: &str = "aurora";
pub const SETTINGS_FILE_NAME: &str = "settings.json";

/// Config dir used before the project was renamed to aurora; still read
/// (never written) during migration.
const PRE_RENAME_CONFIG_DIR_NAME: &str = "legion-kb-rgb";

/// Same serde shape as the old app's `persist::Settings`, so a migrated file
/// parses without conversion. `effects` keeps its historical name.
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct Settings {
    pub profiles: Vec<Profile>,
    pub effects: Vec<CustomEffect>,
    #[serde(alias = "ui_state")]
    pub current_profile: Profile,
}

pub fn settings_file_path() -> Option<PathBuf> {
    let config_dir = dirs::config_dir()?;

    let mut path = config_dir;
    path.push(CONFIG_DIR_NAME);
    path.push(SETTINGS_FILE_NAME);
    Some(path)
}

impl Settings {
    /// Load settings from the XDG path, falling back to a one-time migration
    /// from legacy locations, falling back to defaults.
    pub fn load_or_migrate() -> Self {
        let Some(path) = settings_file_path() else {
            eprintln!("settings: no config directory available, starting with defaults");
            return Self::default();
        };

        if path.is_file() {
            return Self::load_from(&path);
        }

        let Some(legacy_path) = find_legacy_settings_file() else {
            eprintln!("settings: no settings file found, starting fresh at {}", path.display());
            return Self::default();
        };

        eprintln!("settings: migrating legacy settings from {} to {}", legacy_path.display(), path.display());
        let settings = Self::load_from(&legacy_path);
        settings.save_to(&path);
        settings
    }

    fn load_from(path: &Path) -> Self {
        let read_result = fs::read_to_string(path);

        let contents = match read_result {
            Ok(contents) => contents,
            Err(error) => {
                eprintln!("settings: could not read {}: {error}, using defaults", path.display());
                return Self::default();
            }
        };

        match serde_json::from_str(&contents) {
            Ok(settings) => settings,
            Err(error) => {
                eprintln!("settings: could not parse {}: {error}, using defaults", path.display());
                preserve_corrupt_file(path);
                Self::default()
            }
        }
    }

    /// Save to the XDG path, creating the directory if needed. Failures are
    /// logged, not fatal: losing persistence must not kill the lights.
    pub fn save(&self) {
        let Some(path) = settings_file_path() else {
            eprintln!("settings: no config directory available, cannot save");
            return;
        };

        self.save_to(&path);
    }

    fn save_to(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let create_result = fs::create_dir_all(parent);
            if let Err(error) = create_result {
                eprintln!("settings: could not create {}: {error}", parent.display());
                return;
            }
        }

        let serialized = match serde_json::to_string_pretty(self) {
            Ok(serialized) => serialized,
            Err(error) => {
                eprintln!("settings: could not serialize settings: {error}");
                return;
            }
        };

        // Write to a sibling temp file and rename so a crash mid-write can
        // never leave a half-written settings file behind.
        let temp_path = path.with_extension("json.tmp");
        let write_result = fs::write(&temp_path, serialized);
        if let Err(error) = write_result {
            eprintln!("settings: could not write {}: {error}", temp_path.display());
            return;
        }

        let rename_result = fs::rename(&temp_path, path);
        if let Err(error) = rename_result {
            eprintln!("settings: could not move {} into place: {error}", temp_path.display());
        }
    }
}

/// Keep a copy of an unparseable settings file so a daemon bug or a manual
/// edit gone wrong can be recovered from instead of silently overwritten.
fn preserve_corrupt_file(path: &Path) {
    let backup_path = path.with_extension("json.invalid");
    let copy_result = fs::copy(path, &backup_path);
    match copy_result {
        Ok(_) => eprintln!("settings: kept the unparseable file at {}", backup_path.display()),
        Err(error) => eprintln!("settings: could not back up the unparseable file: {error}"),
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
