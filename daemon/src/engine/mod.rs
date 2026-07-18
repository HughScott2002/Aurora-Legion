//! The effect engine: owns the keyboard and runs one effect at a time on a
//! dedicated thread. Transplanted from the old `app/src/manager` module,
//! with three deliberate changes:
//!
//! - no single-instance guard (the daemon's socket bind is the guard),
//! - bounded channels instead of unbounded ones,
//! - driver errors are recorded instead of unwrapped, so an unplugged
//!   keyboard degrades into re-acquisition instead of a dead thread.

use crossbeam_channel::{Receiver, Sender};
use aurora_protocol::{
    custom_effect::{CustomEffect, EffectType},
    effects::{Direction, Effects},
    profile::{self, Profile, COLOR_BYTE_COUNT},
};
use legion_rgb_driver::{BaseEffects, Keyboard, SPEED_RANGE};
use rand::{rng, rngs::ThreadRng};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    thread::JoinHandle,
    time::Duration,
};

mod effects;

/// How many queued engine messages we allow. The engine drains the queue and
/// only acts on the newest message, so a small bound is plenty; the sender
/// blocks briefly if the engine is busy stopping the current effect.
const MESSAGE_QUEUE_CAPACITY: usize = 4;

/// Sleep between queue polls while no effect is running.
const IDLE_POLL_MS: u64 = 20;

/// The speed range software effects accept (hardware effects use the
/// driver's `SPEED_RANGE`).
pub const SOFTWARE_SPEED_RANGE: std::ops::RangeInclusive<u8> = 1..=10;

#[derive(Debug)]
enum Message {
    Profile { profile: Profile },
    CustomEffect { effect: CustomEffect },
    Exit,
}

/// Handle to the engine thread.
pub struct EffectManager {
    tx: Sender<Message>,
    inner_handle: Option<JoinHandle<()>>,
    stop_signals: StopSignals,
    device_error: Arc<AtomicBool>,
}

/// Runs on the engine thread; owns the keyboard.
struct Inner {
    keyboard: Keyboard,
    rx: Receiver<Message>,
    stop_signals: StopSignals,
    device_error: Arc<AtomicBool>,
}

impl EffectManager {
    /// Takes an already-acquired keyboard. Acquisition (with retry and error
    /// classification) lives in `crate::keyboard`, not here.
    pub fn new(keyboard: Keyboard, stop_signals: StopSignals) -> Self {
        let (tx, rx) = crossbeam_channel::bounded::<Message>(MESSAGE_QUEUE_CAPACITY);
        let device_error = Arc::new(AtomicBool::new(false));

        let mut inner = Inner {
            keyboard,
            rx,
            stop_signals: stop_signals.clone(),
            device_error: device_error.clone(),
        };

        let inner_handle = thread::spawn(move || loop {
            // Drain the queue; only the newest message matters because each
            // message fully replaces the keyboard state.
            let mut newest: Option<Message> = None;
            for message in inner.rx.try_iter() {
                newest = Some(message);
            }

            match newest {
                Some(Message::Profile { profile }) => {
                    inner.set_profile(profile);
                }
                Some(Message::CustomEffect { effect }) => {
                    inner.play_custom_effect(&effect);
                }
                Some(Message::Exit) => {
                    break;
                }
                None => {
                    thread::sleep(Duration::from_millis(IDLE_POLL_MS));
                }
            }
        });

        Self {
            tx,
            inner_handle: Some(inner_handle),
            stop_signals,
            device_error,
        }
    }

    /// True once any driver call failed; the daemon reacts by dropping this
    /// engine and re-entering keyboard acquisition.
    pub fn has_device_error(&self) -> bool {
        self.device_error.load(Ordering::SeqCst)
    }

    pub fn set_profile(&self, profile: Profile) {
        self.stop_signals.store_true();
        // Blocking send is safe: the queue only fills while the previous
        // effect is still winding down, which the stop signal bounds to a
        // few tens of milliseconds.
        let send_result = self.tx.send(Message::Profile { profile });
        if send_result.is_err() {
            eprintln!("engine: dropped profile update, engine thread is gone");
        }
    }

    pub fn play_custom_effect(&self, effect: CustomEffect) {
        self.stop_signals.store_true();
        let send_result = self.tx.send(Message::CustomEffect { effect });
        if send_result.is_err() {
            eprintln!("engine: dropped custom effect, engine thread is gone");
        }
    }

    pub fn shutdown(mut self) {
        self.stop_signals.store_true();
        let send_result = self.tx.send(Message::Exit);
        if send_result.is_err() {
            eprintln!("engine: exit message not delivered, engine thread is gone");
        }

        if let Some(handle) = self.inner_handle.take() {
            let join_result = handle.join();
            if join_result.is_err() {
                eprintln!("engine: engine thread panicked before shutdown");
            }
        }
    }
}

impl Drop for EffectManager {
    fn drop(&mut self) {
        let _ = self.tx.send(Message::Exit);
    }
}

impl Inner {
    fn set_profile(&mut self, mut profile: Profile) {
        self.stop_signals.store_false();
        let mut rng = rng();

        if profile.effect.is_built_in() {
            let clamped_speed = clamp_hardware_speed(profile.speed);
            if !self.write_speed(clamped_speed) {
                return;
            }
        } else {
            // All software effects rely on rapidly switching a static color.
            if !self.write_effect(BaseEffects::Static) {
                return;
            }
        }

        let brightness_payload = profile.brightness as u8 + 1;
        if !self.write_brightness(brightness_payload) {
            return;
        }

        self.apply_effect(&mut profile, &mut rng);
        self.stop_signals.store_false();
    }

    fn apply_effect(&mut self, profile: &mut Profile, rng: &mut ThreadRng) {
        match profile.effect {
            Effects::Static => {
                if !self.write_colors(&profile.rgb_array()) {
                    return;
                }
                self.write_effect(BaseEffects::Static);
            }
            Effects::Breath => {
                if !self.write_colors(&profile.rgb_array()) {
                    return;
                }
                self.write_effect(BaseEffects::Breath);
            }
            Effects::Smooth => {
                self.write_effect(BaseEffects::Smooth);
            }
            Effects::Wave => {
                let effect = match profile.direction {
                    Direction::Left => BaseEffects::LeftWave,
                    Direction::Right => BaseEffects::RightWave,
                };
                self.write_effect(effect);
            }
            Effects::Lightning => effects::lightning::play(self, profile, rng),
            Effects::AmbientLight { mut fps, mut saturation_boost } => {
                fps = fps.clamp(1, 60);
                saturation_boost = saturation_boost.clamp(0.0, 1.0);
                effects::ambient::play(self, fps, saturation_boost);
            }
            Effects::SmoothWave { mode, clean_with_black } => {
                profile.rgb_zones = profile::arr_to_zones([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 0, 255]);
                effects::swipe::play(self, profile, mode, clean_with_black);
            }
            Effects::Swipe { mode, clean_with_black } => effects::swipe::play(self, profile, mode, clean_with_black),
            Effects::Disco => effects::disco::play(self, profile, rng),
            Effects::Christmas => effects::christmas::play(self, rng),
            Effects::Fade => effects::fade::play(self, profile),
            Effects::Temperature => effects::temperature::play(self),
            Effects::Ripple => effects::ripple::play(self, profile),
        }
    }

    fn play_custom_effect(&mut self, custom_effect: &CustomEffect) {
        self.stop_signals.store_false();

        loop {
            for step in &custom_effect.effect_steps {
                if !self.write_brightness(step.brightness) {
                    return;
                }

                let step_written = match step.step_type {
                    EffectType::Set => self.write_colors(&step.rgb_array),
                    EffectType::Transition => self.write_transition(&step.rgb_array, step.steps, step.delay_between_steps),
                };
                if !step_written {
                    return;
                }

                if self.stop_signals.manager_stop_signal.load(Ordering::SeqCst) {
                    return;
                }
                thread::sleep(Duration::from_millis(step.sleep));
            }
            if !custom_effect.should_loop {
                break;
            }
        }
    }

    // --- Driver-call wrappers -------------------------------------------
    //
    // Every keyboard write in the engine goes through one of these. On
    // failure they record the device error and trip the stop signals so the
    // running effect unwinds quickly; callers must treat `false` as "stop
    // the effect now".

    fn record_device_error(&self, operation: &str, error: &legion_rgb_driver::error::Error) {
        eprintln!("engine: keyboard {operation} failed: {error}");
        self.device_error.store(true, Ordering::SeqCst);
        self.stop_signals.store_true();
    }

    fn write_effect(&mut self, effect: BaseEffects) -> bool {
        match self.keyboard.set_effect(effect) {
            Ok(()) => true,
            Err(error) => {
                self.record_device_error("set_effect", &error);
                false
            }
        }
    }

    fn write_speed(&mut self, speed: u8) -> bool {
        debug_assert!(SPEED_RANGE.contains(&speed));
        match self.keyboard.set_speed(speed) {
            Ok(()) => true,
            Err(error) => {
                self.record_device_error("set_speed", &error);
                false
            }
        }
    }

    fn write_brightness(&mut self, brightness: u8) -> bool {
        match self.keyboard.set_brightness(brightness) {
            Ok(()) => true,
            Err(error) => {
                self.record_device_error("set_brightness", &error);
                false
            }
        }
    }

    fn write_colors(&mut self, colors: &[u8; COLOR_BYTE_COUNT]) -> bool {
        match self.keyboard.set_colors_to(colors) {
            Ok(()) => true,
            Err(error) => {
                self.record_device_error("set_colors_to", &error);
                false
            }
        }
    }

    fn write_solid_colors(&mut self, color: [u8; 3]) -> bool {
        match self.keyboard.solid_set_colors_to(color) {
            Ok(()) => true,
            Err(error) => {
                self.record_device_error("solid_set_colors_to", &error);
                false
            }
        }
    }

    fn write_zone(&mut self, zone_index: u8, color: [u8; 3]) -> bool {
        match self.keyboard.set_zone_by_index(zone_index, color) {
            Ok(()) => true,
            Err(error) => {
                self.record_device_error("set_zone_by_index", &error);
                false
            }
        }
    }

    fn write_transition(&mut self, colors: &[u8; COLOR_BYTE_COUNT], steps: u8, delay_between_steps_ms: u64) -> bool {
        match self.keyboard.transition_colors_to(colors, steps, delay_between_steps_ms) {
            Ok(()) => true,
            Err(error) => {
                self.record_device_error("transition_colors_to", &error);
                false
            }
        }
    }
}

fn clamp_hardware_speed(speed: u8) -> u8 {
    let min = *SPEED_RANGE.start();
    let max = *SPEED_RANGE.end();
    speed.clamp(min, max)
}

#[derive(Clone)]
pub struct StopSignals {
    pub manager_stop_signal: Arc<AtomicBool>,
    pub keyboard_stop_signal: Arc<AtomicBool>,
}

impl StopSignals {
    pub fn new() -> Self {
        Self {
            manager_stop_signal: Arc::new(AtomicBool::new(false)),
            keyboard_stop_signal: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn store_true(&self) {
        self.keyboard_stop_signal.store(true, Ordering::SeqCst);
        self.manager_stop_signal.store(true, Ordering::SeqCst);
    }

    pub fn store_false(&self) {
        self.keyboard_stop_signal.store(false, Ordering::SeqCst);
        self.manager_stop_signal.store(false, Ordering::SeqCst);
    }
}
