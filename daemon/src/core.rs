//! The daemon core: a single thread that owns the settings, the daemon
//! state and the effect engine. Every mutation — IPC request, hotkey press,
//! device failure — arrives here as a [`Command`], so there is exactly one
//! place where state changes and exactly one place that broadcasts them.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender};
use aurora_protocol::{
    ipc::{DaemonState, ErrorKind, Event, EventEnvelope, KeyboardStatus, Request, Response, ResponseEnvelope},
    profile::Profile,
};
use legion_rgb_driver::{HARDWARE_SLOT_OFF, HARDWARE_SLOT_RANGE};

use crate::{
    engine::{EffectManager, StopSignals, SOFTWARE_SPEED_RANGE},
    keyboard::{self, AcquireOutcome},
    settings::Settings,
};

/// Tick used while something is pending: keyboard acquisition in progress
/// or a debounced settings save waiting to happen.
const TICK_BUSY_MS: u64 = 250;

/// Tick used when the keyboard is healthy and nothing is pending. The only
/// work on this tick is checking the device-error flag, so it can be slow;
/// signals and IPC arrive as commands and wake the loop immediately.
const TICK_IDLE_MS: u64 = 2000;

/// The live profile is saved this long after the last change, so a GUI
/// slider drag does not write the file on every wiggle.
const SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

/// Upper bound on custom effect length, so one bad file cannot balloon the
/// settings file and every state broadcast with it.
const MAX_CUSTOM_EFFECT_STEPS: usize = 4096;

/// Apply the final logical slot only after Fn+Space input has been quiet
/// long enough for the EC's native transition to finish. Every event still
/// advances the logical slot; only redundant intermediate writes are
/// coalesced.
const HARDWARE_SLOT_SETTLE_DELAY: Duration = Duration::from_millis(250);

/// Diagnostic only (`AURORA_TRACE`): how long after a slot write to sample
/// the EC counter. Long enough that the engine has sent its report and a
/// late controller transition would have finished. Reads never move the
/// counter, so sampling cannot change behavior.
const SLOT_TRACE_DELAY: Duration = Duration::from_millis(500);

/// Commands the core accepts. Keep this the only way to mutate daemon state.
pub enum Command {
    Ipc {
        envelope_id: u64,
        request: Request,
        out_tx: Sender<Outbound>,
    },
    CycleProfile,
    /// The Fn+Space "light profile change" WMI event fired; sent by the
    /// slot watcher thread. The core advances Aurora's logical slot once.
    HardwareSlotEvent,
    /// SIGTERM/SIGINT arrived; sent by the signal listener thread so the
    /// core wakes immediately instead of on its next tick.
    ShutdownSignal,
}

/// A line queued for one client connection; the connection's writer thread
/// serializes it.
#[derive(Debug, Clone)]
pub enum Outbound {
    Response(ResponseEnvelope),
    Event(EventEnvelope),
}

pub struct Core {
    settings: Settings,
    current_profile: Profile,
    custom_effect_playing: Option<String>,
    keyboard_status: KeyboardStatus,

    engine: Option<EffectManager>,
    stop_signals: StopSignals,

    /// Aurora's logical slot: 1..=3, HARDWARE_SLOT_OFF, or None while no
    /// keyboard slot is active. The EC counter is not trusted after
    /// Aurora's first write because writes move it without a WMI event.
    hardware_slot: Option<u8>,
    /// Deadline for applying the final logical slot after an Fn+Space
    /// burst. A later event replaces this deadline.
    hardware_slot_apply_at: Option<Instant>,
    /// Read handle for the EC counter, kept past acquisition only so
    /// tracing can sample it. The counter is still never used for slot
    /// identity after the first read.
    slot_reader: Option<legion_rgb_driver::SlotReader>,
    /// Deadline for a diagnostic counter sample after a slot write.
    slot_trace_at: Option<Instant>,

    subscribers: Vec<Sender<Outbound>>,

    settings_dirty: bool,
    last_change_at: Instant,

    acquire_attempt_count: u32,
    next_acquire_at: Instant,

    shutdown_requested: bool,
}

pub fn run(command_rx: &Receiver<Command>, shutdown_flag: &Arc<AtomicBool>) {
    let mut settings = Settings::load_or_migrate();
    let current_profile = settings.current_profile.clone();

    settings.normalize_hardware_slots();

    let mut core = Core {
        settings,
        current_profile,
        custom_effect_playing: None,
        keyboard_status: KeyboardStatus::Searching,
        engine: None,
        stop_signals: StopSignals::new(),
        hardware_slot: None,
        hardware_slot_apply_at: None,
        slot_reader: None,
        slot_trace_at: None,
        subscribers: Vec::new(),
        settings_dirty: false,
        last_change_at: Instant::now(),
        acquire_attempt_count: 0,
        next_acquire_at: Instant::now(),
        shutdown_requested: false,
    };

    loop {
        if shutdown_flag.load(Ordering::SeqCst) || core.shutdown_requested {
            break;
        }

        core.check_device_error();
        core.try_acquire_keyboard_if_due();

        match command_rx.recv_timeout(core.next_tick_timeout()) {
            Ok(command) => core.handle_command(command),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }

        core.apply_hardware_slot_if_due();
        core.trace_slot_counter_if_due();
        core.save_settings_if_due();
    }

    core.shutdown();
}

impl Core {
    // --- Keyboard lifecycle ---------------------------------------------

    fn check_device_error(&mut self) {
        let engine_failed = match &self.engine {
            Some(engine) => engine.has_device_error(),
            None => false,
        };

        if !engine_failed {
            return;
        }

        eprintln!("core: keyboard lost, re-entering acquisition");
        if let Some(engine) = self.engine.take() {
            engine.shutdown();
        }

        self.stop_signals = StopSignals::new();
        self.hardware_slot = None;
        self.hardware_slot_apply_at = None;
        self.slot_reader = None;
        self.slot_trace_at = None;
        self.keyboard_status = KeyboardStatus::Searching;
        self.acquire_attempt_count = 0;
        self.next_acquire_at = Instant::now();
        self.broadcast_state();
    }

    fn try_acquire_keyboard_if_due(&mut self) {
        if self.engine.is_some() {
            return;
        }
        if Instant::now() < self.next_acquire_at {
            return;
        }

        let outcome = keyboard::try_acquire(&self.stop_signals);
        self.acquire_attempt_count += 1;
        self.next_acquire_at = Instant::now() + keyboard::backoff_delay(self.acquire_attempt_count);

        match outcome {
            AcquireOutcome::Acquired(keyboard) => {
                // This is the only trusted counter read. It must happen
                // before the keyboard moves into the engine and before
                // Aurora sends its first lighting report.
                let slot_reader = keyboard.slot_reader();
                let slot_read_result = slot_reader.read_slot_counter();
                let initial_slot = match slot_read_result {
                    Ok(value) if is_settled_slot_value(value) => {
                        // Logged even on the happy path: a wrong-but-settled
                        // start value drifts every later slot for the life of
                        // this acquisition, and nothing else would show it.
                        eprintln!("core: startup slot counter read {value}");
                        Some(value)
                    }
                    Ok(value) => {
                        let fallback_slot = *HARDWARE_SLOT_RANGE.start();
                        eprintln!(
                            "core: startup slot counter was not settled ({value:#04x}), \
                             falling back to slot {fallback_slot}"
                        );
                        Some(fallback_slot)
                    }
                    Err(error) => {
                        let fallback_slot = *HARDWARE_SLOT_RANGE.start();
                        eprintln!(
                            "core: startup slot counter read failed ({error}), \
                             falling back to slot {fallback_slot}"
                        );
                        Some(fallback_slot)
                    }
                };

                let engine = EffectManager::new(*keyboard, self.stop_signals.clone());
                self.engine = Some(engine);
                self.keyboard_status = KeyboardStatus::Connected;
                self.custom_effect_playing = None;
                self.hardware_slot = initial_slot;
                self.slot_reader = Some(slot_reader);
                self.apply_profile_for_acquired_slot();
                self.broadcast_state();
            }
            AcquireOutcome::Failed(status) => {
                // Only broadcast on transitions so a missing keyboard does
                // not spam subscribers every ten seconds.
                if status != self.keyboard_status {
                    eprintln!("core: keyboard not acquired: {status:?}");
                    self.keyboard_status = status;
                    self.broadcast_state();
                }
            }
        }
    }

    // --- Command handling ------------------------------------------------

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::Ipc { envelope_id, request, out_tx } => {
                let response = self.handle_request(request, &out_tx);
                let envelope = ResponseEnvelope { id: envelope_id, resp: response };
                let send_result = out_tx.send(Outbound::Response(envelope));
                if send_result.is_err() {
                    // Client vanished between request and response; harmless.
                }
            }
            Command::CycleProfile => {
                let response = self.cycle_profile();
                if let Response::Error { message, .. } = response {
                    eprintln!("core: hotkey profile cycle failed: {message}");
                }
            }
            Command::HardwareSlotEvent => {
                self.advance_hardware_slot();
            }
            Command::ShutdownSignal => {
                self.shutdown_requested = true;
            }
        }
    }

    /// Slow tick when the keyboard is healthy and nothing is pending; fast
    /// tick while acquiring the keyboard or holding an unsaved change.
    fn next_tick_timeout(&self) -> Duration {
        let mut timeout = if self.settings_dirty || self.engine.is_none() {
            Duration::from_millis(TICK_BUSY_MS)
        } else {
            Duration::from_millis(TICK_IDLE_MS)
        };

        let deadlines = [self.hardware_slot_apply_at, self.slot_trace_at];
        for deadline in deadlines {
            let Some(deadline) = deadline else {
                continue;
            };
            let deadline_timeout = deadline.saturating_duration_since(Instant::now());
            if deadline_timeout < timeout {
                timeout = deadline_timeout;
            }
        }

        timeout
    }

    fn handle_request(&mut self, request: Request, out_tx: &Sender<Outbound>) -> Response {
        match request {
            Request::Hello { protocol_version } => {
                // Answer regardless of the client's version; the client
                // decides whether to proceed. The log line is for the case
                // where the client carries on anyway and later requests fail.
                if protocol_version != aurora_protocol::ipc::PROTOCOL_VERSION {
                    eprintln!(
                        "core: client speaks protocol v{protocol_version}, daemon speaks v{}",
                        aurora_protocol::ipc::PROTOCOL_VERSION
                    );
                }
                Response::Hello {
                    protocol_version: aurora_protocol::ipc::PROTOCOL_VERSION,
                    daemon_version: env!("CARGO_PKG_VERSION").to_string(),
                }
            }
            Request::GetState => Response::State { state: self.state_snapshot() },
            Request::Subscribe => {
                self.subscribers.push(out_tx.clone());
                Response::Ok
            }
            Request::SetProfile { profile } => self.set_profile(profile),
            Request::PlayCustomEffect { effect } => self.play_custom_effect(effect),
            Request::StopCustomEffect => self.stop_custom_effect(),
            Request::ListProfiles => Response::Profiles {
                profiles: self.settings.profiles.clone(),
            },
            Request::AddProfile { profile } => self.add_profile(profile),
            Request::DeleteProfile { name } => self.delete_profile(&name),
            Request::SwitchProfile { name } => self.switch_profile(&name),
            Request::CycleProfile => self.cycle_profile(),
            Request::ListCustomEffects => Response::CustomEffects {
                effects: self.settings.effects.clone(),
            },
            Request::AddCustomEffect { effect } => self.add_custom_effect(effect),
            Request::DeleteCustomEffect { name } => self.delete_custom_effect(&name),
            Request::Shutdown => {
                self.shutdown_requested = true;
                Response::Ok
            }
        }
    }

    // A client request while the backlight is off (hardware slot 4) is
    // still applied: an explicit "light the keyboard like this" beats the
    // remembered off state. It is not stored into any slot; only the
    // lighting slots 1..=3 remember profiles.
    fn set_profile(&mut self, profile: Profile) -> Response {
        if let Some(rejection) = validate_profile(&profile) {
            return rejection;
        }

        self.current_profile = profile.clone();
        self.custom_effect_playing = None;
        self.hardware_slot_apply_at = None;
        if let Some(engine) = &self.engine {
            engine.set_profile(profile);
        }

        self.store_current_into_active_slot();
        self.mark_changed();
        self.broadcast_state();
        Response::Ok
    }

    fn play_custom_effect(&mut self, effect: aurora_protocol::custom_effect::CustomEffect) -> Response {
        if effect.effect_steps.is_empty() {
            return error_response(ErrorKind::InvalidRequest, "custom effect has no steps");
        }
        if effect.effect_steps.len() > MAX_CUSTOM_EFFECT_STEPS {
            return error_response(
                ErrorKind::InvalidRequest,
                &format!("custom effect has {} steps, the limit is {MAX_CUSTOM_EFFECT_STEPS}", effect.effect_steps.len()),
            );
        }

        let display_name = match &effect.name {
            Some(name) => name.clone(),
            None => "Unnamed".to_string(),
        };

        if let Some(engine) = &self.engine {
            engine.play_custom_effect(effect);
        }

        self.hardware_slot_apply_at = None;
        self.custom_effect_playing = Some(display_name);
        self.broadcast_state();
        Response::Ok
    }

    fn stop_custom_effect(&mut self) -> Response {
        self.custom_effect_playing = None;
        self.hardware_slot_apply_at = None;
        if let Some(engine) = &self.engine {
            engine.set_profile(self.current_profile.clone());
        }
        self.broadcast_state();
        Response::Ok
    }

    fn add_profile(&mut self, mut profile: Profile) -> Response {
        let Some(name) = profile.name.clone() else {
            return error_response(ErrorKind::InvalidRequest, "profile needs a name to be saved");
        };
        if name.is_empty() {
            return error_response(ErrorKind::InvalidRequest, "profile name is empty");
        }
        if let Some(rejection) = validate_profile(&profile) {
            return rejection;
        }

        profile.name = Some(name.clone());

        let mut replaced = false;
        for saved in &mut self.settings.profiles {
            if saved.name.as_deref() == Some(name.as_str()) {
                *saved = profile.clone();
                replaced = true;
                break;
            }
        }
        if !replaced {
            self.settings.profiles.push(profile);
        }

        self.mark_changed();
        self.broadcast_state();
        Response::Ok
    }

    fn delete_profile(&mut self, name: &str) -> Response {
        let position = self.settings.profiles.iter().position(|saved| saved.name.as_deref() == Some(name));

        match position {
            Some(index) => {
                self.settings.profiles.remove(index);
                self.mark_changed();
                self.broadcast_state();
                Response::Ok
            }
            None => error_response(ErrorKind::NoSuchProfile, &format!("no saved profile called '{name}'")),
        }
    }

    fn switch_profile(&mut self, name: &str) -> Response {
        let mut found: Option<Profile> = None;
        for saved in &self.settings.profiles {
            if saved.name.as_deref() == Some(name) {
                found = Some(saved.clone());
                break;
            }
        }

        match found {
            Some(profile) => self.set_profile(profile),
            None => error_response(ErrorKind::NoSuchProfile, &format!("no saved profile called '{name}'")),
        }
    }

    fn cycle_profile(&mut self) -> Response {
        let profile_count = self.settings.profiles.len();
        if profile_count == 0 {
            return error_response(ErrorKind::NoSuchProfile, "no saved profiles to cycle through");
        }

        let current_name = self.current_profile.name.clone();

        let mut current_index: Option<usize> = None;
        for (index, saved) in self.settings.profiles.iter().enumerate() {
            if saved.name == current_name {
                current_index = Some(index);
                break;
            }
        }

        let next_index = match current_index {
            Some(index) => (index + 1) % profile_count,
            // Current profile is unsaved; start from the first saved one.
            None => 0,
        };

        let next_profile = self.settings.profiles[next_index].clone();
        self.set_profile(next_profile)
    }

    fn add_custom_effect(&mut self, effect: aurora_protocol::custom_effect::CustomEffect) -> Response {
        let Some(name) = effect.name.clone() else {
            return error_response(ErrorKind::InvalidRequest, "custom effect needs a name to be saved");
        };
        if name.is_empty() {
            return error_response(ErrorKind::InvalidRequest, "custom effect name is empty");
        }
        if effect.effect_steps.is_empty() {
            return error_response(ErrorKind::InvalidRequest, "custom effect has no steps");
        }
        if effect.effect_steps.len() > MAX_CUSTOM_EFFECT_STEPS {
            return error_response(
                ErrorKind::InvalidRequest,
                &format!("custom effect has {} steps, the limit is {MAX_CUSTOM_EFFECT_STEPS}", effect.effect_steps.len()),
            );
        }

        let mut replaced = false;
        for saved in &mut self.settings.effects {
            if saved.name.as_deref() == Some(name.as_str()) {
                *saved = effect.clone();
                replaced = true;
                break;
            }
        }
        if !replaced {
            self.settings.effects.push(effect);
        }

        self.mark_changed();
        self.broadcast_state();
        Response::Ok
    }

    fn delete_custom_effect(&mut self, name: &str) -> Response {
        let position = self.settings.effects.iter().position(|saved| saved.name.as_deref() == Some(name));

        match position {
            Some(index) => {
                self.settings.effects.remove(index);
                self.mark_changed();
                self.broadcast_state();
                Response::Ok
            }
            None => error_response(ErrorKind::NoSuchProfile, &format!("no saved custom effect called '{name}'")),
        }
    }

    // --- Hardware slots (Fn+Space) ---------------------------------------

    /// First write after acquisition. Respect what the EC is showing: a
    /// known lighting slot applies that slot's remembered profile, off
    /// writes nothing, and an unknown slot (no reader) falls back to the
    /// last live profile, which is the pre-slot-tracking behavior.
    ///
    fn apply_profile_for_acquired_slot(&mut self) {
        match self.hardware_slot {
            Some(slot) if HARDWARE_SLOT_RANGE.contains(&slot) => {
                let slot_position = (slot - 1) as usize;
                let Some(slot_profile) = self.settings.hardware_slot_profiles.get(slot_position) else {
                    // Cannot happen after normalize_hardware_slots.
                    eprintln!("core: no remembered profile for hardware slot {slot}");
                    return;
                };

                eprintln!("core: keyboard acquired on hardware slot {slot}, applying its remembered profile");
                let slot_profile = slot_profile.clone();
                self.current_profile = slot_profile.clone();
                if let Some(engine) = &self.engine {
                    engine.set_profile(slot_profile);
                }
                self.mark_changed();
            }
            Some(_off) => {
                eprintln!("core: keyboard acquired with the backlight off, not writing");
            }
            None => {
                eprintln!("core: keyboard acquired, applying current profile");
                let profile = self.current_profile.clone();
                if let Some(engine) = &self.engine {
                    engine.set_profile(profile);
                }
            }
        }
    }

    /// Aurora owns the visible Fn+Space sequence after startup. The EC
    /// counter cannot identify later slots because every Aurora lighting
    /// write can move it without emitting a WMI event.
    fn advance_hardware_slot(&mut self) {
        let Some(current_slot) = self.hardware_slot else {
            eprintln!("core: hardware slot event ignored because startup slot is unknown");
            return;
        };

        let event_at = Instant::now();
        let Some((new_slot, apply_at)) = schedule_hardware_slot_apply(current_slot, event_at) else {
            eprintln!("core: hardware slot event ignored because slot {current_slot} is invalid");
            return;
        };

        self.hardware_slot = Some(new_slot);
        self.hardware_slot_apply_at = Some(apply_at);
        self.custom_effect_playing = None;

        if new_slot == HARDWARE_SLOT_OFF {
            eprintln!("core: hardware backlight off (slot {new_slot}) selected");
            return;
        }

        eprintln!("core: hardware slot {new_slot} selected");
    }

    /// Apply only the final slot in an Fn+Space burst. Waiting outside the
    /// command handler keeps the core responsive and ensures Aurora writes
    /// after the EC has finished its last native transition.
    fn apply_hardware_slot_if_due(&mut self) {
        let Some(apply_at) = self.hardware_slot_apply_at else {
            return;
        };
        if Instant::now() < apply_at {
            return;
        }

        self.hardware_slot_apply_at = None;
        let Some(slot) = self.hardware_slot else {
            return;
        };

        if slot == HARDWARE_SLOT_OFF {
            eprintln!("core: hardware backlight off (slot {slot}), applying blackout");
            if let Some(engine) = &self.engine {
                engine.set_profile(Profile::default());
            }
            self.schedule_slot_trace();
            self.broadcast_state();
            return;
        }

        if !HARDWARE_SLOT_RANGE.contains(&slot) {
            eprintln!("core: pending hardware slot {slot} is invalid");
            return;
        }

        let slot_position = (slot - 1) as usize;
        let Some(slot_profile) = self.settings.hardware_slot_profiles.get(slot_position) else {
            eprintln!("core: no remembered profile for hardware slot {slot}");
            return;
        };

        eprintln!("core: hardware slot {slot} settled, applying its remembered profile");
        let slot_profile = slot_profile.clone();
        self.current_profile = slot_profile.clone();
        if let Some(engine) = &self.engine {
            engine.set_profile(slot_profile);
        }
        self.schedule_slot_trace();
        self.mark_changed();
        self.broadcast_state();
    }

    /// Arm a diagnostic counter sample after a slot write. No-op unless
    /// `AURORA_TRACE` is set.
    fn schedule_slot_trace(&mut self) {
        if !legion_rgb_driver::trace_enabled() {
            return;
        }

        self.slot_trace_at = Some(Instant::now() + SLOT_TRACE_DELAY);
    }

    /// Diagnostic only: read the EC counter once, a fixed delay after
    /// Aurora's own slot write, and log it beside the logical slot. This is
    /// the only view of what the controller did after our report landed.
    /// It samples the counter, not the visible lighting, so a mismatch is
    /// evidence and not proof.
    fn trace_slot_counter_if_due(&mut self) {
        let Some(trace_at) = self.slot_trace_at else {
            return;
        };
        if Instant::now() < trace_at {
            return;
        }

        self.slot_trace_at = None;

        let Some(slot_reader) = &self.slot_reader else {
            return;
        };
        let Some(logical_slot) = self.hardware_slot else {
            return;
        };

        match slot_reader.read_slot_counter() {
            Ok(counter) => eprintln!("trace: {}ms after slot write, logical slot {logical_slot}, EC counter {counter}", SLOT_TRACE_DELAY.as_millis()),
            Err(error) => eprintln!("trace: {}ms after slot write, logical slot {logical_slot}, EC counter unreadable ({error})", SLOT_TRACE_DELAY.as_millis()),
        }
    }

    /// Every lighting change lands in whichever EC slot is active, so the
    /// slot's remembered profile follows the live profile.
    fn store_current_into_active_slot(&mut self) {
        let Some(active_slot) = self.hardware_slot else {
            return;
        };
        if !HARDWARE_SLOT_RANGE.contains(&active_slot) {
            return; // Off (or unknown) stores nothing.
        }

        let slot_position = (active_slot - 1) as usize;
        if let Some(slot_entry) = self.settings.hardware_slot_profiles.get_mut(slot_position) {
            *slot_entry = self.current_profile.clone();
        }
    }

    // --- State + persistence ---------------------------------------------

    fn state_snapshot(&self) -> DaemonState {
        DaemonState {
            keyboard: self.keyboard_status.clone(),
            current: self.current_profile.clone(),
            custom_effect_playing: self.custom_effect_playing.clone(),
            profiles: self.settings.profiles.clone(),
            custom_effects: self.settings.effects.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            hardware_slot: self.hardware_slot,
        }
    }

    fn broadcast_state(&mut self) {
        let state = self.state_snapshot();
        let envelope = EventEnvelope {
            event: Event::StateChanged { state },
        };

        // Send to every subscriber; drop the ones whose connection is gone
        // or whose queue is full (a stuck client must not stall the core —
        // it can reconnect and re-sync with GetState).
        let mut alive: Vec<Sender<Outbound>> = Vec::with_capacity(self.subscribers.len());
        for subscriber in self.subscribers.drain(..) {
            let send_result = subscriber.try_send(Outbound::Event(envelope.clone()));
            match send_result {
                Ok(()) => alive.push(subscriber),
                Err(crossbeam_channel::TrySendError::Full(_)) => {
                    eprintln!("core: dropping subscriber with a full queue");
                }
                Err(crossbeam_channel::TrySendError::Disconnected(_)) => {}
            }
        }
        self.subscribers = alive;
    }

    fn mark_changed(&mut self) {
        self.settings.current_profile = self.current_profile.clone();
        self.settings_dirty = true;
        self.last_change_at = Instant::now();
    }

    fn save_settings_if_due(&mut self) {
        if !self.settings_dirty {
            return;
        }
        if self.last_change_at.elapsed() < SAVE_DEBOUNCE {
            return;
        }

        self.settings.save();
        self.settings_dirty = false;
    }

    fn shutdown(mut self) {
        eprintln!("core: shutting down");

        if let Some(engine) = self.engine.take() {
            engine.shutdown();
        }

        self.settings.current_profile = self.current_profile.clone();
        self.settings.save();
    }
}

/// The counter values the EC settles on: a lighting slot or off. Anything
/// else is a mid-switch transient reading.
fn is_settled_slot_value(value: u8) -> bool {
    HARDWARE_SLOT_RANGE.contains(&value) || value == HARDWARE_SLOT_OFF
}

fn next_hardware_slot(current_slot: u8) -> Option<u8> {
    if HARDWARE_SLOT_RANGE.contains(&current_slot) {
        if current_slot < *HARDWARE_SLOT_RANGE.end() {
            return Some(current_slot + 1);
        }
        return Some(HARDWARE_SLOT_OFF);
    }

    if current_slot == HARDWARE_SLOT_OFF {
        return Some(*HARDWARE_SLOT_RANGE.start());
    }

    None
}

fn schedule_hardware_slot_apply(current_slot: u8, event_at: Instant) -> Option<(u8, Instant)> {
    let new_slot = next_hardware_slot(current_slot)?;
    let apply_at = event_at + HARDWARE_SLOT_SETTLE_DELAY;
    Some((new_slot, apply_at))
}

/// Returns `Some(error response)` when the profile is out of range.
fn validate_profile(profile: &Profile) -> Option<Response> {
    if !SOFTWARE_SPEED_RANGE.contains(&profile.speed) {
        return Some(error_response(
            ErrorKind::InvalidRequest,
            &format!("speed {} outside {:?}", profile.speed, SOFTWARE_SPEED_RANGE),
        ));
    }

    if let aurora_protocol::effects::Effects::AmbientLight { fps, saturation_boost } = profile.effect {
        if !(1..=60).contains(&fps) {
            return Some(error_response(ErrorKind::InvalidRequest, &format!("ambient fps {fps} outside 1..=60")));
        }
        if !(0.0..=1.0).contains(&saturation_boost) {
            return Some(error_response(ErrorKind::InvalidRequest, "ambient saturation boost outside 0.0..=1.0"));
        }
    }

    None
}

fn error_response(kind: ErrorKind, message: &str) -> Response {
    Response::Error {
        kind,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{next_hardware_slot, schedule_hardware_slot_apply, HARDWARE_SLOT_SETTLE_DELAY};

    #[test]
    fn hardware_slot_cycle_is_logical() {
        let expected_slots = [2, 3, 4, 1];
        let mut current_slot = 1;

        for expected_slot in expected_slots {
            assert_eq!(next_hardware_slot(current_slot), Some(expected_slot));
            current_slot = expected_slot;
        }

        assert_eq!(next_hardware_slot(0), None);
    }

    #[test]
    fn rapid_events_advance_each_slot_and_delay_the_final_apply() {
        let first_event_at = Instant::now();
        let Some((second_slot, first_apply_at)) = schedule_hardware_slot_apply(1, first_event_at) else {
            panic!("slot 1 should advance");
        };

        let second_event_at = first_event_at + Duration::from_millis(50);
        let Some((third_slot, second_apply_at)) = schedule_hardware_slot_apply(second_slot, second_event_at) else {
            panic!("slot 2 should advance");
        };

        assert_eq!(second_slot, 2);
        assert_eq!(third_slot, 3);
        assert_eq!(first_apply_at, first_event_at + HARDWARE_SLOT_SETTLE_DELAY);
        assert_eq!(second_apply_at, second_event_at + HARDWARE_SLOT_SETTLE_DELAY);
        assert!(second_apply_at > first_apply_at);
    }
}
