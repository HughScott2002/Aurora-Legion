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

/// A full battery, in the percent the kernel reports.
pub const FULL_CHARGE_PERCENT: u8 = 100;

/// How much charge one zone of the battery gauge stands for. Four zones
/// over a full battery, so 25 points each.
pub const ZONE_CHARGE_SPAN: u32 = FULL_CHARGE_PERCENT as u32 / ZONE_COUNT as u32;

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

    /// The color payload for a battery gauge at `percent`: this slot's own
    /// colors, dimmed zone by zone so the lit part of the keyboard is the
    /// part of the charge that is left.
    ///
    /// The four zones split the battery evenly and empty right to left, so
    /// zone 4 stands for the top 25 points and zone 1 for the last 25. A
    /// zone only partly inside the remaining charge is drawn at that
    /// fraction of its color, which is what lets the gauge move
    /// continuously instead of stepping four times over a whole discharge.
    ///
    /// The effect chooses nothing here. Every color comes from the slot the
    /// user set up, and all this does is decide how much of each survives.
    ///
    /// Lives in the protocol crate because two places have to agree on the
    /// answer: the daemon writes it to the keyboard, and the app draws it
    /// in the preview. Two implementations would eventually disagree, and
    /// the disagreement would look like a rendering bug.
    pub fn battery_gauge_array(&self, percent: u8) -> [u8; COLOR_BYTE_COUNT] {
        let full = self.rgb_array();
        let mut dimmed: [u8; COLOR_BYTE_COUNT] = [0; COLOR_BYTE_COUNT];

        // Kernel input reaches this, so clamp rather than assert.
        let charge = u32::from(percent.min(FULL_CHARGE_PERCENT));

        for zone_index in 0..ZONE_COUNT {
            // Charge this zone starts filling at: 0, 25, 50, 75.
            let zone_floor = zone_index as u32 * ZONE_CHARGE_SPAN;
            let charge_in_zone = charge.saturating_sub(zone_floor).min(ZONE_CHARGE_SPAN);

            let byte_offset = zone_index * COLOR_CHANNELS_PER_ZONE;
            for channel_index in 0..COLOR_CHANNELS_PER_ZONE {
                let channel = u32::from(full[byte_offset + channel_index]);
                let scaled = channel * charge_in_zone / ZONE_CHARGE_SPAN;
                dimmed[byte_offset + channel_index] = scaled as u8;
            }
        }

        dimmed
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
pub const DEFAULT_SLOT_COLORS: [[u8; COLOR_CHANNELS_PER_ZONE]; SLOT_COUNT] =
    [[255, 0, 0], [0, 255, 0], [0, 0, 255]];

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
        let legacy: LegacyProfileFile =
            serde_json::from_str(&contents).change_context(LoadProfileError)?;

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

        assert_eq!(
            profile.slots[0].rgb_array(),
            [255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0]
        );
        assert_eq!(
            profile.slots[1].rgb_array(),
            [0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0]
        );
        assert_eq!(
            profile.slots[2].rgb_array(),
            [0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 255]
        );
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

        let parsed: LegacyProfileFile =
            serde_json::from_str(legacy).expect("legacy file should parse");
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

    #[test]
    fn a_full_battery_leaves_every_zone_alone() {
        let lighting = Lighting::solid([255, 128, 0]);

        assert_eq!(
            lighting.battery_gauge_array(100),
            lighting.rgb_array(),
            "a full gauge is the user's own lighting, untouched"
        );
    }

    #[test]
    fn an_empty_battery_leaves_nothing_lit() {
        let lighting = Lighting::solid([255, 128, 0]);

        assert_eq!(lighting.battery_gauge_array(0), [0; COLOR_BYTE_COUNT]);
    }

    /// The gauge empties right to left, so the rightmost zone is the first
    /// to go dark and the leftmost is the last.
    #[test]
    fn the_gauge_empties_from_the_right() {
        let lighting = Lighting::solid([200, 200, 200]);

        let half = lighting.battery_gauge_array(50);
        assert_eq!(&half[0..3], &[200, 200, 200], "zone 1 is still full");
        assert_eq!(&half[3..6], &[200, 200, 200], "zone 2 is still full");
        assert_eq!(&half[6..9], &[0, 0, 0], "zone 3 is out");
        assert_eq!(&half[9..12], &[0, 0, 0], "zone 4 went first");
    }

    /// Whole-zone steps would move four times over a whole discharge, which
    /// is not a gauge. The zone straddling the charge line is dimmed.
    #[test]
    fn the_zone_on_the_charge_line_is_part_lit() {
        let lighting = Lighting::solid([100, 0, 0]);

        // 60%: zones 1 and 2 full, zone 3 ten points into its span of 25.
        let gauge = lighting.battery_gauge_array(60);
        assert_eq!(&gauge[0..3], &[100, 0, 0]);
        assert_eq!(&gauge[3..6], &[100, 0, 0]);
        assert_eq!(&gauge[6..9], &[40, 0, 0]);
        assert_eq!(&gauge[9..12], &[0, 0, 0]);
    }

    /// The effect dims the user's colors and picks none of its own, so a
    /// zone the user turned off stays off at every charge.
    #[test]
    fn the_gauge_keeps_the_users_own_colors() {
        let mut lighting = Lighting::default();
        lighting.rgb_zones[0].rgb = [10, 20, 30];
        lighting.rgb_zones[1].rgb = [40, 50, 60];
        lighting.rgb_zones[2].rgb = [70, 80, 90];
        lighting.rgb_zones[3].rgb = [255, 255, 255];
        lighting.rgb_zones[3].enabled = false;

        let gauge = lighting.battery_gauge_array(100);
        assert_eq!(&gauge[0..3], &[10, 20, 30]);
        assert_eq!(&gauge[3..6], &[40, 50, 60]);
        assert_eq!(&gauge[6..9], &[70, 80, 90]);
        assert_eq!(&gauge[9..12], &[0, 0, 0], "a disabled zone stays dark");
    }

    /// The percent comes from sysfs, so a nonsense value must clamp rather
    /// than wrap the arithmetic or panic.
    #[test]
    fn a_charge_above_full_is_clamped() {
        let lighting = Lighting::solid([255, 255, 255]);

        assert_eq!(
            lighting.battery_gauge_array(255),
            lighting.battery_gauge_array(100)
        );
    }

    /// Structural equality is what change detection needs. The old
    /// discriminant-only PartialEq made these compare equal, so an fps edit
    /// was silently dropped.
    #[test]
    fn lighting_equality_sees_inner_effect_settings() {
        let slow = Lighting {
            effect: Effects::AmbientLight {
                fps: 10,
                saturation_boost: 0.0,
            },
            ..Default::default()
        };
        let fast = Lighting {
            effect: Effects::AmbientLight {
                fps: 60,
                saturation_boost: 0.0,
            },
            ..Default::default()
        };

        assert_ne!(slow, fast);
        assert!(slow.effect.same_variant(fast.effect));
    }
}
