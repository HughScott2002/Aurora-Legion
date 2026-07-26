use error::{RangeError, RangeErrorKind, Result};
use hidapi::{HidApi, HidDevice};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
    thread,
    time::Duration,
};

pub mod error;

const KNOWN_DEVICE_INFOS: [(u16, u16, u16, u16); 11] = [
    (0x048d, 0xc995, 0xff89, 0x00cc), // 2024 Pro
    (0x048d, 0xc994, 0xff89, 0x00cc), // 2024
    (0x048d, 0xc993, 0xff89, 0x00cc), // 2024 LOQ
    (0x048d, 0xc985, 0xff89, 0x00cc), // 2023 Pro
    (0x048d, 0xc984, 0xff89, 0x00cc), // 2023
    (0x048d, 0xc983, 0xff89, 0x00cc), // 2023 LOQ
    (0x048d, 0xc975, 0xff89, 0x00cc), // 2022
    (0x048d, 0xc973, 0xff89, 0x00cc), // 2022 Ideapad
    (0x048d, 0xc965, 0xff89, 0x00cc), // 2021
    (0x048d, 0xc963, 0xff89, 0x00cc), // 2021 Ideapad
    (0x048d, 0xc955, 0xff89, 0x00cc), // 2020
];

pub const SPEED_RANGE: std::ops::RangeInclusive<u8> = 1..=4;
pub const BRIGHTNESS_RANGE: std::ops::RangeInclusive<u8> = 1..=2;
pub const ZONE_RANGE: std::ops::RangeInclusive<u8> = 0..=3;

/// The EC's hardware lighting slots, cycled by Fn+Space. The controller
/// stores a profile per slot; no HID command is known to select or program
/// a specific slot, only to read the counter (see `SlotReader`).
pub const HARDWARE_SLOT_RANGE: std::ops::RangeInclusive<u8> = 1..=3;

/// Slot counter value the EC reports while the backlight is off.
pub const HARDWARE_SLOT_OFF: u8 = 4;

/// One feature report on this device: the 0xCC report ID plus 32 payload
/// bytes, in both the SET and GET directions.
const FEATURE_REPORT_BYTES: usize = 33;

/// Report ID of the lighting feature report.
const FEATURE_REPORT_ID: u8 = 0xcc;

pub enum BaseEffects {
    Static,
    Breath,
    Smooth,
    LeftWave,
    RightWave,
}

pub struct LightingState {
    effect_type: BaseEffects,
    speed: u8,
    brightness: u8,
    rgb_values: [u8; 12],
}

/// The one open handle to the controller. Shared between the effect
/// engine (writes) and the daemon core's [`SlotReader`] (reads): with the
/// libusb backend the HID interface can only be claimed once, so a second
/// `open_device` fails and sharing is the only option. Each driver call
/// holds the lock for a single feature-report transfer.
type SharedHidDevice = Arc<Mutex<HidDevice>>;

/// Locks the shared handle. A poisoned lock (a panic on the other thread
/// mid-call) still yields a usable device: the handle is just an fd and
/// carries no invariant that a panic could break.
fn lock_hid_device(device: &SharedHidDevice) -> MutexGuard<'_, HidDevice> {
    match device.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub struct Keyboard {
    keyboard_hid: SharedHidDevice,
    current_state: LightingState,
    stop_signal: Arc<AtomicBool>,
}

#[allow(dead_code)]
impl Keyboard {
    fn build_payload(&self) -> Result<[u8; 33]> {
        let keyboard_state = &self.current_state;

        if !SPEED_RANGE.contains(&keyboard_state.speed) {
            return Err(RangeError { kind: RangeErrorKind::Speed }.into());
        }
        if !BRIGHTNESS_RANGE.contains(&keyboard_state.brightness) {
            return Err(RangeError { kind: RangeErrorKind::Brightness }.into());
        }

        let mut payload: [u8; 33] = [0; 33];
        payload[0] = 0xcc;
        payload[1] = 0x16;
        payload[2] = match keyboard_state.effect_type {
            BaseEffects::Static => 0x01,
            BaseEffects::Breath => 0x03,
            BaseEffects::Smooth => 0x06,
            BaseEffects::LeftWave => {
                payload[19] = 0x1;
                0x04
            }
            BaseEffects::RightWave => {
                payload[18] = 0x1;
                0x04
            }
        };

        payload[3] = keyboard_state.speed;
        payload[4] = keyboard_state.brightness;

        if let BaseEffects::Static | BaseEffects::Breath = keyboard_state.effect_type {
            payload[5..(12 + 5)].copy_from_slice(&keyboard_state.rgb_values[..12]);
        };

        Ok(payload)
    }

    pub fn refresh(&mut self) -> Result<()> {
        let payload = self.build_payload()?;

        // Propagate instead of unwrapping: this is the call that fails when
        // the keyboard is unplugged, and callers need to see that error.
        let device = lock_hid_device(&self.keyboard_hid);
        device.send_feature_report(&payload)?;

        Ok(())
    }

    /// A read handle onto this keyboard's shared HID device, for the EC
    /// slot counter. Take it before the `Keyboard` moves into the engine.
    pub fn slot_reader(&self) -> SlotReader {
        SlotReader {
            device_hid: Arc::clone(&self.keyboard_hid),
        }
    }

    pub fn set_effect(&mut self, effect: BaseEffects) -> Result<()> {
        self.current_state.effect_type = effect;
        self.refresh()?;

        Ok(())
    }

    pub fn set_speed(&mut self, speed: u8) -> Result<()> {
        if !SPEED_RANGE.contains(&speed) {
            return Err(RangeError { kind: RangeErrorKind::Speed }.into());
        }

        self.current_state.speed = speed;
        self.refresh()?;

        Ok(())
    }

    pub fn set_brightness(&mut self, brightness: u8) -> Result<()> {
        if !BRIGHTNESS_RANGE.contains(&brightness) {
            return Err(RangeError { kind: RangeErrorKind::Brightness }.into());
        }
        let brightness = brightness.clamp(BRIGHTNESS_RANGE.min().unwrap(), BRIGHTNESS_RANGE.max().unwrap());
        self.current_state.brightness = brightness;
        self.refresh()?;

        Ok(())
    }

    pub fn set_zone_by_index(&mut self, zone_index: u8, new_values: [u8; 3]) -> Result<()> {
        if !ZONE_RANGE.contains(&zone_index) {
            return Err(RangeError { kind: RangeErrorKind::Zone }.into());
        }

        for (i, _) in new_values.iter().enumerate() {
            let full_index = (zone_index * 3 + i as u8) as usize;
            self.current_state.rgb_values[full_index] = new_values[i];
        }
        self.refresh()?;

        Ok(())
    }

    pub fn set_colors_to(&mut self, new_values: &[u8; 12]) -> Result<()> {
        if let BaseEffects::Static | BaseEffects::Breath = self.current_state.effect_type {
            for (i, _) in new_values.iter().enumerate() {
                self.current_state.rgb_values[i] = new_values[i];
            }
            self.refresh()?;
        }

        Ok(())
    }

    pub fn solid_set_colors_to(&mut self, new_values: [u8; 3]) -> Result<()> {
        if let BaseEffects::Static | BaseEffects::Breath = self.current_state.effect_type {
            for i in (0..12).step_by(3) {
                self.current_state.rgb_values[i] = new_values[0];
                self.current_state.rgb_values[i + 1] = new_values[1];
                self.current_state.rgb_values[i + 2] = new_values[2];
            }
            self.refresh()?;
        }

        Ok(())
    }

    pub fn transition_colors_to(&mut self, target_colors: &[u8; 12], steps: u8, delay_between_steps: u64) -> Result<()> {
        if let BaseEffects::Static | BaseEffects::Breath = self.current_state.effect_type {
            let mut new_values = self.current_state.rgb_values.map(f32::from);
            let mut color_differences: [f32; 12] = [0.0; 12];
            for index in 0..12 {
                color_differences[index] = (f32::from(target_colors[index]) - f32::from(self.current_state.rgb_values[index])) / f32::from(steps);
            }
            if !self.stop_signal.load(Ordering::SeqCst) {
                for _step_num in 1..=steps {
                    if self.stop_signal.load(Ordering::SeqCst) {
                        break;
                    }
                    for (index, _) in color_differences.iter().enumerate() {
                        new_values[index] += color_differences[index];
                    }
                    self.current_state.rgb_values = new_values.map(|val| val as u8);

                    self.refresh()?;
                    thread::sleep(Duration::from_millis(delay_between_steps));
                }
                self.set_colors_to(target_colors)?;
            }
        }

        Ok(())
    }
}

fn find_known_device(api: &HidApi) -> Result<&hidapi::DeviceInfo> {
    api.device_list()
        .find(|d| {
            #[cfg(target_os = "windows")]
            {
                let info_tuple = (d.vendor_id(), d.product_id(), d.usage_page(), d.usage());

                KNOWN_DEVICE_INFOS.iter().any(|known| known == &info_tuple)
            }

            #[cfg(target_os = "linux")]
            {
                let info_tuple = (d.vendor_id(), d.product_id());

                KNOWN_DEVICE_INFOS.iter().any(|known| (known.0, known.1) == info_tuple)
            }
        })
        .ok_or(error::Error::DeviceNotFound)
}

/// A read view onto the keyboard's shared HID handle, used to read the
/// EC's hardware slot counter from the daemon core thread. It shares the
/// engine's handle (see [`SharedHidDevice`]); each read holds the lock for
/// one GET_FEATURE transfer, so it never starves the effect engine.
pub struct SlotReader {
    device_hid: SharedHidDevice,
}

impl SlotReader {
    /// Reads the EC's slot counter byte, raw. Settled values are
    /// [`HARDWARE_SLOT_RANGE`] (the stored lighting slots) and
    /// [`HARDWARE_SLOT_OFF`]; the EC also reports transient values outside
    /// that range while it is switching, so callers must be ready to see
    /// and re-poll past them (observed live on a 2023 Pro). A short report
    /// is a [`RangeErrorKind::Slot`] error; a device failure is a HID error.
    pub fn read_slot_counter(&self) -> Result<u8> {
        let mut report: [u8; FEATURE_REPORT_BYTES] = [0; FEATURE_REPORT_BYTES];
        report[0] = FEATURE_REPORT_ID;

        let read_count = {
            let device = lock_hid_device(&self.device_hid);
            device.get_feature_report(&mut report)?
        };
        if read_count < 2 {
            return Err(RangeError { kind: RangeErrorKind::Slot }.into());
        }

        Ok(report[1])
    }
}

pub fn get_keyboard(stop_signal: Arc<AtomicBool>) -> Result<Keyboard> {
    let api: HidApi = HidApi::new()?;

    let info = find_known_device(&api)?;

    let keyboard_hid: SharedHidDevice = Arc::new(Mutex::new(info.open_device(&api)?));
    let current_state: LightingState = LightingState {
        effect_type: BaseEffects::Static,
        speed: 1,
        brightness: 1,
        rgb_values: [0; 12],
    };

    let mut keyboard = Keyboard {
        keyboard_hid,
        current_state,
        stop_signal,
    };

    keyboard.refresh()?;
    Ok(keyboard)
}

pub fn find_possible_keyboards() -> Result<Vec<String>> {
    let api: HidApi = HidApi::new()?;

    let mut list = api
        .device_list()
        .filter(|d| d.vendor_id() == 0x048d)
        .map(|d| format!("{:#06x}:{:#06x}", d.vendor_id(), d.product_id()))
        .collect::<Vec<String>>();

    list.dedup();
    Ok(list)
}
