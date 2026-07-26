//! The daemon core: a single thread that owns the settings, the daemon
//! state and the effect engine. Every mutation — IPC request, hotkey press,
//! device failure — arrives here as a [`Command`], so there is exactly one
//! place where state changes and exactly one place that broadcasts them.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender};
use aurora_protocol::{
    ipc::{DaemonState, ErrorKind, Event, EventEnvelope, KeyboardStatus, Request, Response, ResponseEnvelope},
    profile::Profile,
};
use legion_rgb_driver::{SlotReader, HARDWARE_SLOT_OFF, HARDWARE_SLOT_RANGE};

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

/// After the Fn+Space WMI event the EC updates its slot counter
/// asynchronously and offers no completion signal, so the readback is a
/// bounded poll: at most this many reads, one every
/// [`SLOT_POLL_INTERVAL_MS`] (the upper bound maniac103's daemon observed
/// in practice; see docs/research/ite8295-hardware-profiles.md).
///
/// The counter is read ONLY inside this event-anchored window (plus once
/// at acquisition). It cannot be watched passively: the daemon's own
/// lighting writes move the counter without firing the WMI event
/// (observed live on a 2023 Pro), so a periodic check reads write noise
/// and re-applies profiles in an endless loop.
const SLOT_POLL_MAX_READS: u32 = 100;
const SLOT_POLL_INTERVAL_MS: u64 = 10;

/// Commands the core accepts. Keep this the only way to mutate daemon state.
pub enum Command {
    Ipc {
        envelope_id: u64,
        request: Request,
        out_tx: Sender<Outbound>,
    },
    CycleProfile,
    /// The Fn+Space "light profile change" WMI event fired; sent by the
    /// slot watcher thread. The core reacts by re-reading the EC's slot
    /// counter and applying the remembered profile for the new slot.
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

    /// Read-only handle for the EC's hardware slot counter; acquired and
    /// dropped alongside the engine.
    slot_reader: Option<SlotReader>,
    /// Last observed EC slot: 1..=3, HARDWARE_SLOT_OFF, or None when
    /// unknown (no keyboard, or the counter is unreadable).
    hardware_slot: Option<u8>,
    /// True while slot reads are erroring, so the log gets one line per
    /// failure streak instead of one per read.
    slot_read_failing: bool,

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

    let hardware_slot_count = *HARDWARE_SLOT_RANGE.end() as usize;
    settings.normalize_hardware_slots(hardware_slot_count, &current_profile);

    let mut core = Core {
        settings,
        current_profile,
        custom_effect_playing: None,
        keyboard_status: KeyboardStatus::Searching,
        engine: None,
        stop_signals: StopSignals::new(),
        slot_reader: None,
        hardware_slot: None,
        slot_read_failing: false,
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
        self.slot_reader = None;
        self.hardware_slot = None;
        self.slot_read_failing = false;
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
                // The reader shares the keyboard's HID handle; take it
                // before the keyboard moves into the engine.
                self.slot_reader = Some(keyboard.slot_reader());
                let engine = EffectManager::new(*keyboard, self.stop_signals.clone());
                self.engine = Some(engine);
                self.keyboard_status = KeyboardStatus::Connected;
                self.custom_effect_playing = None;
                // Learn which EC slot the keyboard is on BEFORE writing
                // anything, so the right slot's profile is applied and no
                // slot's memory is overwritten by a stale live profile. A
                // transient counter value reads as "unknown".
                let raw_slot = self.read_slot_once();
                self.hardware_slot = raw_slot.filter(|value| is_settled_slot_value(*value));
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
                self.sync_hardware_slot();
            }
            Command::ShutdownSignal => {
                self.shutdown_requested = true;
            }
        }
    }

    /// Slow tick when the keyboard is healthy and nothing is pending; fast
    /// tick while acquiring the keyboard or holding an unsaved change.
    fn next_tick_timeout(&self) -> Duration {
        if self.settings_dirty {
            return Duration::from_millis(TICK_BUSY_MS);
        }
        if self.engine.is_none() {
            return Duration::from_millis(TICK_BUSY_MS);
        }
        Duration::from_millis(TICK_IDLE_MS)
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

        self.custom_effect_playing = Some(display_name);
        self.broadcast_state();
        Response::Ok
    }

    fn stop_custom_effect(&mut self) -> Response {
        self.custom_effect_playing = None;
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
    /// The counter read here may carry noise from a previous daemon's
    /// writes (see `sync_hardware_slot`); the worst case is one wrong
    /// slot's profile applied once at startup, which the next Fn+Space
    /// press corrects. No loop is possible because nothing re-reads the
    /// counter outside event windows.
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

    fn read_slot_once(&mut self) -> Option<u8> {
        let Some(reader) = &self.slot_reader else {
            return None;
        };

        // Any error is treated as transient and the reading skipped: the
        // controller STALLs GET_FEATURE while the EC is mid-switch
        // (observed live on a 2023 Pro). Device health is the engine's
        // call; a dead device fails the engine's writes, which rebuilds
        // everything including this reader.
        match reader.read_slot_counter() {
            Ok(counter_value) => {
                if self.slot_read_failing {
                    self.slot_read_failing = false;
                    eprintln!("core: hardware slot reads recovered");
                }
                Some(counter_value)
            }
            Err(error) => {
                if !self.slot_read_failing {
                    self.slot_read_failing = true;
                    eprintln!("core: hardware slot read failed, will keep trying: {error}");
                }
                None
            }
        }
    }

    /// React to the Fn+Space WMI event: work out which slot the EC landed
    /// on and apply that slot's remembered profile (or stop writing when
    /// the user turned the backlight off).
    ///
    /// The counter's absolute value is only meaningful here, anchored to
    /// the event: the daemon's own writes move it silently, so it is
    /// compared against a fresh read taken now, never against the
    /// displayed slot. The EC updates the counter shortly *after* the
    /// event, so the whole exchange is one bounded poll: first successful
    /// read is the pre-press (or already settled) value, then wait for
    /// the counter to move off it; if it never moves within the budget,
    /// the first read already was the settled post-press value.
    fn sync_hardware_slot(&mut self) {
        if self.slot_reader.is_none() {
            return;
        }

        let mut read_count: u32 = 0;

        // First successful read; failures here are the EC mid-switch.
        let mut first_value: Option<u8> = None;
        while read_count < SLOT_POLL_MAX_READS {
            first_value = self.read_slot_once();
            read_count += 1;
            if first_value.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(SLOT_POLL_INTERVAL_MS));
        }
        let Some(first_value) = first_value else {
            return; // Unreadable for the whole budget.
        };

        // Wait for the counter to move off the first reading.
        let mut settled_value = first_value;
        while read_count < SLOT_POLL_MAX_READS {
            thread::sleep(Duration::from_millis(SLOT_POLL_INTERVAL_MS));
            let raw_value = self.read_slot_once();
            read_count += 1;
            let Some(value) = raw_value else {
                continue;
            };
            settled_value = value;
            if value != first_value {
                break;
            }
        }

        if !is_settled_slot_value(settled_value) {
            // Mid-switch transient at budget end; the next event retries.
            eprintln!("core: EC slot counter did not settle (last read {settled_value:#04x})");
            return;
        }

        let new_slot = settled_value;
        if Some(new_slot) == self.hardware_slot {
            return;
        }

        self.hardware_slot = Some(new_slot);

        if new_slot == HARDWARE_SLOT_OFF {
            // The user turned the backlight off with Fn+Space; stop any
            // software effect so the daemon does not fight the EC, and do
            // not write to the device.
            eprintln!("core: hardware backlight off (slot {new_slot})");
            self.custom_effect_playing = None;
            self.stop_signals.store_true();
            self.broadcast_state();
            return;
        }

        let slot_position = (new_slot - 1) as usize;
        let Some(slot_profile) = self.settings.hardware_slot_profiles.get(slot_position) else {
            // Cannot happen after normalize_hardware_slots; be safe anyway.
            eprintln!("core: no remembered profile for hardware slot {new_slot}");
            return;
        };

        eprintln!("core: hardware slot {new_slot} active, applying its remembered profile");
        let slot_profile = slot_profile.clone();
        self.current_profile = slot_profile.clone();
        self.custom_effect_playing = None;
        if let Some(engine) = &self.engine {
            engine.set_profile(slot_profile);
        }

        self.mark_changed();
        self.broadcast_state();
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
