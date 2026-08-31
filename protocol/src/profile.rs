//! The lighting model.
//!
//! A [`Profile`] is the named, saveable thing. It owns one [`Lighting`] per
//! Fn+Space slot, so switching slots on the keyboard moves between three
//! looks that belong to the same profile.
//!
//! Aurora before 0.24 had no such split: a profile *was* one lighting
//! configuration, and per-slot lighting lived in a separate settings field.
//! Profile files written by those versions still load; see
//! [`Profile::load_profile`].

use std::path::Path;

use crate::{
    effects::{Brightness, Direction, Effects},
    storage::StorageTrait,
};

use error_stack::{Result, ResultExt};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const ZONE_COUNT: usize = 4;
pub const COLOR_CHANNELS_PER_ZONE: usize = 3;
pub const COLOR_BYTE_COUNT: usize = ZONE_COUNT * COLOR_CHANNELS_PER_ZONE;

/// Fn+Space lighting slots per profile. The controller cycles these three
/// plus an off position; see [`crate::ipc::SlotSelection`].
pub const SLOT_COUNT: usize = 3;

/// Longest profile name accepted. Names reach every client inside a state
/// broadcast, so they are bounded like every other payload.
pub const MAX_NAME_BYTES: usize = 64;

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct KeyboardZone {
    pub rgb: [u8; COLOR_CHANNELS_PER_ZONE],
    pub enabled: bool,
}

impl Default for KeyboardZone {
    fn default() -> Self {
        Self {
            rgb: Default::default(),
            enabled: true,
        }
    }
}

pub type Zones = [KeyboardZone; ZONE_COUNT];

/// What one slot shows: the whole of the old `Profile` minus its name.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Lighting {
    pub rgb_zones: Zones,
    pub effect: Effects,
    pub direction: Direction,
    pub speed: u8,
    pub brightness: Brightness,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            rgb_zones: Zones::default(),
            effect: Effects::default(),
            direction: Direction::default(),
            speed: 1,
            brightness: Brightness::default(),
        }
    }
}

impl Lighting {
    /// Flatten the four zones into the 12-byte color payload the keyboard
    /// expects. Disabled zones stay black.
    pub fn rgb_array(&self) -> [u8; COLOR_BYTE_COUNT] {
        let mut colors: [u8; COLOR_BYTE_COUNT] = [0; COLOR_BYTE_COUNT];

        for (zone_index, zone) in self.rgb_zones.iter().enumerate() {
            if !zone.enabled {
                continue;
            }

            let byte_offset = zone_index * COLOR_CHANNELS_PER_ZONE;
            colors[byte_offset] = zone.rgb[0];
            colors[byte_offset + 1] = zone.rgb[1];
            colors[byte_offset + 2] = zone.rgb[2];
        }

        colors
    }

    /// A slot filled with one solid color on every zone. Used for the
    /// red, green and blue slot defaults.
    pub fn solid(color: [u8; COLOR_CHANNELS_PER_ZONE]) -> Self {
        let mut lighting = Self::default();

        for zone in &mut lighting.rgb_zones {
            zone.rgb = color;
        }

        lighting
    }
}

/// A named profile: one lighting configuration per Fn+Space slot.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Profile {
    pub name: Option<String>,
    pub slots: [Lighting; SLOT_COUNT],
}

/// Slot colors a fresh profile starts with, so cycling with Fn+Space shows
/// which slot is active without reading any status output.
pub const DEFAULT_SLOT_COLORS: [[u8; COLOR_CHANNELS_PER_ZONE]; SLOT_COUNT] = [
    [255, 0, 0],
    [0, 255, 0],
    [0, 0, 255],
];

impl Default for Profile {
    fn default() -> Self {
        let mut slots: [Lighting; SLOT_COUNT] = Default::default();

        for (slot_index, slot) in slots.iter_mut().enumerate() {
            *slot = Lighting::solid(DEFAULT_SLOT_COLORS[slot_index]);
        }

        Self { name: None, slots }
    }
}

impl Profile {
    /// Every slot showing the same lighting. This is what a pre-0.24
    /// profile becomes: activating it looks the same on whichever slot the
    /// keyboard happens to be on, which is how it behaved before slots
    /// existed.
    pub fn from_uniform_lighting(name: Option<String>, lighting: Lighting) -> Self {
        let slots: [Lighting; SLOT_COUNT] = std::array::from_fn(|_slot_index| lighting.clone());
        Self { name, slots }
    }

    /// The slot whose lighting equals `lighting`, if exactly one does.
    /// Migration uses this to recover which slot was live, since pre-0.24
    /// settings never recorded it.
    pub fn slot_showing(&self, lighting: &Lighting) -> Option<usize> {
        let mut found: Option<usize> = None;

        for (slot_index, slot) in self.slots.iter().enumerate() {
            if slot != lighting {
                continue;
            }
            if found.is_some() {
                return None; // Ambiguous: two slots hold the same lighting.
            }
            found = Some(slot_index);
        }

        found
    }
}

#[derive(Debug, Error)]
#[error("Could not load profile")]
pub struct LoadProfileError;

#[derive(Debug, Error)]
#[error("Could not save profile")]
pub struct SaveProfileError;

/// A profile file written by Aurora 0.23 or earlier: one flat lighting
/// configuration with a name. Kept so `aurora load-profile` never rejects a
/// file a user already has on disk.
#[derive(Deserialize)]
struct LegacyProfileFile {
    name: Option<String>,
    rgb_zones: Zones,
    effect: Effects,
    direction: Direction,
    speed: u8,
    brightness: Brightness,
}

impl LegacyProfileFile {
    fn into_profile(self) -> Profile {
        let lighting = Lighting {
            rgb_zones: self.rgb_zones,
            effect: self.effect,
            direction: self.direction,
            speed: self.speed,
            brightness: self.brightness,
        };

        Profile::from_uniform_lighting(self.name, lighting)
    }
}

impl Profile {
    /// Load a profile file. Current files parse directly; pre-0.24 flat
    /// files are lifted into a profile whose three slots match.
    pub fn load_profile(path: &Path) -> Result<Self, LoadProfileError> {
        let current_result = Self::load(path);
        if let Ok(profile) = current_result {
            return Ok(profile);
        }

        let contents = std::fs::read_to_string(path).change_context(LoadProfileError)?;
        let legacy: LegacyProfileFile = serde_json::from_str(&contents).change_context(LoadProfileError)?;

        Ok(legacy.into_profile())
    }

    pub fn save_profile(&mut self, path: &Path) -> Result<(), SaveProfileError> {
        if self.name.is_none() {
            self.name = Some("Untitled".to_string());
        }
        self.save(path).change_context(SaveProfileError)
    }
}

/// Split a 12-byte color payload into the four keyboard zones, all enabled.
pub fn arr_to_zones(arr: [u8; COLOR_BYTE_COUNT]) -> Zones {
    let mut zones = Zones::default();

    for (zone_index, zone) in zones.iter_mut().enumerate() {
        let byte_offset = zone_index * COLOR_CHANNELS_PER_ZONE;
        zone.rgb = [arr[byte_offset], arr[byte_offset + 1], arr[byte_offset + 2]];
    }

    zones
}

impl StorageTrait<'_> for Profile {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_slots_are_red_green_blue() {
        let profile = Profile::default();

        assert_eq!(profile.slots[0].rgb_array(), [255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0]);
        assert_eq!(profile.slots[1].rgb_array(), [0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0]);
        assert_eq!(profile.slots[2].rgb_array(), [0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 255]);
    }

    /// A profile file from 0.23 or earlier must still load, or users lose
    /// files they saved with `aurora set --save`.
    #[test]
    fn legacy_profile_file_lifts_into_every_slot() {
        let legacy = r#"{
            "name": "old",
            "rgb_zones": [
                {"rgb": [255, 0, 0], "enabled": true},
                {"rgb": [0, 255, 0], "enabled": true},
                {"rgb": [0, 0, 255], "enabled": false},
                {"rgb": [1, 2, 3], "enabled": true}
            ],
            "effect": "Breath",
            "direction": "Right",
            "speed": 3,
            "brightness": "High"
        }"#;

        let parsed: LegacyProfileFile = serde_json::from_str(legacy).expect("legacy file should parse");
        let profile = parsed.into_profile();

        assert_eq!(profile.name.as_deref(), Some("old"));
        for slot in &profile.slots {
            assert_eq!(slot.effect, Effects::Breath);
            assert_eq!(slot.speed, 3);
            assert!(!slot.rgb_zones[2].enabled);
        }
    }

    #[test]
    fn slot_showing_needs_an_unambiguous_match() {
        let profile = Profile::default();

        let green = profile.slots[1].clone();
        assert_eq!(profile.slot_showing(&green), Some(1));

        // Two identical slots cannot identify which one was live.
        let uniform = Profile::from_uniform_lighting(None, Lighting::solid([1, 2, 3]));
        assert_eq!(uniform.slot_showing(&uniform.slots[0].clone()), None);

        // Lighting that no slot holds.
        assert_eq!(profile.slot_showing(&Lighting::solid([9, 9, 9])), None);
    }

    /// Structural equality is what change detection needs. The old
    /// discriminant-only PartialEq made these compare equal, so an fps edit
    /// was silently dropped.
    #[test]
    fn lighting_equality_sees_inner_effect_settings() {
        let slow = Lighting {
            effect: Effects::AmbientLight { fps: 10, saturation_boost: 0.0 },
            ..Default::default()
        };
        let fast = Lighting {
            effect: Effects::AmbientLight { fps: 60, saturation_boost: 0.0 },
            ..Default::default()
        };

        assert_ne!(slow, fast);
        assert!(slow.effect.same_variant(fast.effect));
    }
}
