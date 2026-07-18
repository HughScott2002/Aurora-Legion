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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Profile {
    pub name: Option<String>,
    pub rgb_zones: Zones,
    pub effect: Effects,
    pub direction: Direction,
    pub speed: u8,
    pub brightness: Brightness,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            name: None,
            rgb_zones: Zones::default(),
            effect: Effects::default(),
            direction: Direction::default(),
            speed: 1,
            brightness: Brightness::default(),
        }
    }
}

#[derive(Debug, Error)]
#[error("Could not load profile")]
pub struct LoadProfileError;

#[derive(Debug, Error)]
#[error("Could not save profile")]
pub struct SaveProfileError;

impl Profile {
    pub fn load_profile(path: &Path) -> Result<Self, LoadProfileError> {
        Self::load(path).change_context(LoadProfileError)
    }

    pub fn save_profile(&mut self, path: &Path) -> Result<(), SaveProfileError> {
        if self.name.is_none() {
            self.name = Some("Untitled".to_string());
        }
        self.save(path).change_context(SaveProfileError)
    }

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
