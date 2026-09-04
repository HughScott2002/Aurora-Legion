//! The daemon core: a single thread that owns the settings, the daemon
//! state and the effect engine. Every mutation (IPC request, hotkey press,
//! Fn+Space event, device failure) arrives here as a [`Command`], so there
//! is exactly one place where state changes and exactly one place that
//! broadcasts them.

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use aurora_protocol::{
    custom_effect::CustomEffect,
    effects::{Brightness, Effects},
    ipc::{
        CustomEffectSummary, DaemonState, ErrorKind, Event, EventEnvelope, KeyboardStatus,
        ProfileSummary, Request, Response, ResponseEnvelope, SlotSelection, Subsystem,
        SubsystemState, MAX_CUSTOM_EFFECT_STEPS, MAX_SAVED_CUSTOM_EFFECTS, MAX_SAVED_PROFILES,
    },
    profile::{Lighting, Profile, MAX_NAME_BYTES},
};
use crossbeam_channel::{Receiver, Sender};

use crate::{
    battery,
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

/// Apply the final slot only after Fn+Space input has been quiet long
/// enough for the controller's native transition to finish. Every event
/// still advances the slot; only redundant intermediate writes are
/// coalesced. Writing inside the transition window is what leaves the
/// keyboard dark; see `docs/explanation/fn-space-sync.md`.
const SLOT_SETTLE_DELAY: Duration = Duration::from_millis(250);

/// Diagnostic only (`AURORA_TRACE`): how long after a slot write to sample
/// the controller counter. Reads never move the counter, so sampling
/// cannot change behavior.
const SLOT_TRACE_DELAY: Duration = Duration::from_millis(500);

/// Charge at or below which the keyboard turns red, while the battery is
/// discharging. Above this, or on AC, lighting is whatever the user chose.
const BATTERY_ALERT_PERCENT: u8 = 15;

/// How often the battery is read. This is a deadline checked on the core
/// tick, not a wakeup of its own: `TICK_IDLE_MS` is shorter, so the read
/// always lands within one idle tick of falling due and costs nothing on
/// top of a loop that was already turning. A four-byte sysfs read does not
/// justify a udev netlink listener of the kind `slot_watch` needs.
const BATTERY_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// What the keyboard shows while the battery is low: red at the hardware's
/// low brightness, so the alert looks the same whatever brightness the
/// user's own lighting was using.
const BATTERY_ALERT_COLOR: [u8; 3] = [255, 0, 0];

/// Commands the core accepts. Keep this the only way to mutate daemon state.
pub enum Command {
    Ipc {
        envelope_id: u64,
        request: Request,
        out_tx: Sender<Outbound>,
    },
    CycleProfile,
    /// The Fn+Space "light profile change" event fired; sent by the slot
    /// watcher thread. The core advances its slot once per event.
    HardwareSlotEvent,
    /// SIGTERM/SIGINT arrived; sent by the signal listener thread so the
    /// core wakes immediately instead of on its next tick.
    ShutdownSignal,
    /// An optional subsystem changed state. Sent by the adapter that owns
    /// it, so "Fn+Space does not work here" becomes protocol-visible
    /// instead of a log line nobody reads.
    SubsystemStatus {
        subsystem: Subsystem,
        state: SubsystemState,
    },
}

/// A line queued for one client connection; the connection's writer thread
/// serializes it.
#[derive(Debug, Clone)]
pub enum Outbound {
    Response(ResponseEnvelope),
    Event(EventEnvelope),
}

pub struct Core {
    /// A sender back into this core's own queue, handed to components the
    /// core creates (the effect engine) so they can report state.
    command_tx: Sender<Command>,
    settings: Settings,
    current_profile: Profile,
    /// Which Fn+Space position is live. Aurora's own number after
    /// acquisition: the controller's counter moves in response to Aurora's
    /// writes without raising an event, so it is trusted exactly once.
    active_slot: SlotSelection,
    custom_effect_playing: Option<String>,
    keyboard_status: KeyboardStatus,

    engine: Option<EffectManager>,
    stop_signals: StopSignals,

    /// Deadline for applying the settled slot after an Fn+Space burst. A
    /// later event replaces this deadline; a client slot selection inherits
    /// it rather than cancelling it.
    slot_apply_at: Option<Instant>,
    /// Read handle for the controller counter, kept past acquisition only
    /// so tracing can sample it.
    slot_reader: Option<legion_rgb_driver::SlotReader>,
    /// Deadline for a diagnostic counter sample after a slot write.
    slot_trace_at: Option<Instant>,

    subscribers: Vec<Sender<Outbound>>,

    settings_dirty: bool,
    settings_error: Option<String>,
    last_change_at: Instant,

    acquire_attempt_count: u32,
    next_acquire_at: Instant,

    slot_sync: SubsystemState,
    hotkey: SubsystemState,
    screen_capture: SubsystemState,

    /// The battery directory found at startup, or `None` on a machine
    /// without one. Decided once: batteries do not appear at runtime, and
    /// a desktop must never pay for a read.
    battery_dir: Option<PathBuf>,
    /// Whether the battery is low enough for the alert to want the
    /// keyboard. Whether it actually gets it also depends on the backlight
    /// being on; see [`Core::battery_alert_is_showing`]. Never persisted:
    /// this is a fact about the battery, not a setting.
    battery_alert_active: bool,
    /// The charge last read, or `None` before the first read and on a
    /// machine without a battery. Reported to clients so the app can draw
    /// the Battery effect's gauge; the effect itself reads sysfs directly
    /// on the engine thread and does not use this.
    battery_percent: Option<u8>,
    /// When the battery is next due to be read.
    battery_read_at: Instant,

    shutdown_requested: bool,
}

pub fn run(
    command_tx: &Sender<Command>,
    command_rx: &Receiver<Command>,
    shutdown_flag: &Arc<AtomicBool>,
    battery_dir: Option<PathBuf>,
) {
    let settings = Settings::load_or_migrate();
    let current_profile = settings.current_profile.clone();
    let active_slot = settings.active_slot;

    let mut core = Core {
        command_tx: command_tx.clone(),
        settings,
        current_profile,
        active_slot,
        custom_effect_playing: None,
        keyboard_status: KeyboardStatus::Searching,
        engine: None,
        stop_signals: StopSignals::new(),
        slot_apply_at: None,
        slot_reader: None,
        slot_trace_at: None,
        subscribers: Vec::new(),
        settings_dirty: false,
        settings_error: None,
        last_change_at: Instant::now(),
        acquire_attempt_count: 0,
        next_acquire_at: Instant::now(),
        slot_sync: SubsystemState::Unknown,
        hotkey: SubsystemState::Unknown,
        screen_capture: SubsystemState::Inactive,
        battery_dir,
        battery_alert_active: false,
        battery_percent: None,
        // Read on the first tick rather than after one interval, so a
        // daemon started on a nearly flat battery warns immediately.
        battery_read_at: Instant::now(),
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

        core.apply_slot_if_due();
        core.trace_slot_counter_if_due();
        core.read_battery_if_due();
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
        self.slot_apply_at = None;
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
                // The only trusted counter read. It must happen before the
                // keyboard moves into the engine and before Aurora sends its
                // first lighting report.
                let slot_reader = keyboard.slot_reader();
                let counter_result = slot_reader.read_slot_counter();
                self.active_slot = anchor_slot(self.active_slot, counter_result);

                let engine = EffectManager::new(
                    *keyboard,
                    self.stop_signals.clone(),
                    Some(self.command_tx.clone()),
                    self.battery_dir.clone(),
                );
                self.engine = Some(engine);
                self.slot_reader = Some(slot_reader);
                self.keyboard_status = KeyboardStatus::Connected;
                self.apply_active_slot("keyboard acquired");
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
            Command::Ipc {
                envelope_id,
                request,
                out_tx,
            } => {
                let response = self.handle_request(request, &out_tx);
                let envelope = ResponseEnvelope {
                    id: envelope_id,
                    resp: response,
                };
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
                self.advance_slot();
            }
            Command::ShutdownSignal => {
                self.shutdown_requested = true;
            }
            Command::SubsystemStatus { subsystem, state } => {
                self.set_subsystem_state(subsystem, state);
            }
        }
    }

    /// Store a subsystem's state, broadcasting only on a real change.
    /// Adapters may report the same state repeatedly (a retrying capture
    /// loop, a reconnecting socket); without this compare, each repeat
    /// would be a broadcast to every client.
    fn set_subsystem_state(&mut self, subsystem: Subsystem, state: SubsystemState) {
        let slot = match subsystem {
            Subsystem::SlotSync => &mut self.slot_sync,
            Subsystem::Hotkey => &mut self.hotkey,
            Subsystem::ScreenCapture => &mut self.screen_capture,
        };

        if *slot == state {
            return;
        }

        eprintln!("core: {subsystem:?} is now {state:?}");
        *slot = state;
        self.broadcast_state();
    }

    /// Slow tick when the keyboard is healthy and nothing is pending; fast
    /// tick while acquiring the keyboard or holding an unsaved change.
    fn next_tick_timeout(&self) -> Duration {
        let mut timeout = if self.settings_dirty || self.engine.is_none() {
            Duration::from_millis(TICK_BUSY_MS)
        } else {
            Duration::from_millis(TICK_IDLE_MS)
        };

        let deadlines = [self.slot_apply_at, self.slot_trace_at];
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
                // decides whether to proceed.
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
            Request::GetState => Response::State {
                state: self.state_snapshot(),
            },
            Request::Subscribe => {
                self.subscribers.push(out_tx.clone());
                Response::Ok
            }
            Request::SetProfile { profile } => self.set_profile(profile),
            Request::SetLighting { slot, lighting } => self.set_lighting(slot, lighting),
            Request::SelectSlot { slot } => self.select_slot(slot),
            Request::PlayCustomEffect { effect } => self.play_custom_effect(effect),
            Request::PlayCustomEffectByName { name } => self.play_custom_effect_by_name(&name),
            Request::StopCustomEffect => self.stop_custom_effect(),
            Request::AddProfile { profile } => self.add_profile(profile),
            Request::DeleteProfile { name } => self.delete_profile(&name),
            Request::SwitchProfile { name } => self.switch_profile(&name),
            Request::CycleProfile => self.cycle_profile(),
            Request::AddCustomEffect { effect } => self.add_custom_effect(effect),
            Request::DeleteCustomEffect { name } => self.delete_custom_effect(&name),
            Request::SetBatteryAlert { enabled } => self.set_battery_alert(enabled),
            Request::Shutdown => {
                self.shutdown_requested = true;
                Response::Ok
            }
        }
    }

    // --- Slots ------------------------------------------------------------

    /// The one place lighting reaches the keyboard. Every caller (startup,
    /// Fn+Space, client selection, client edit, profile switch) ends here,
    /// and every call broadcasts, so daemon state and the keyboard cannot
    /// drift apart.
    fn apply_active_slot(&mut self, reason: &str) {
        self.custom_effect_playing = None;
        self.slot_apply_at = None;

        let lighting = self.lighting_to_show();

        eprintln!("core: applying slot {} ({reason})", self.active_slot);
        if let Some(engine) = &self.engine {
            engine.set_lighting(lighting);
        }

        self.schedule_slot_trace();
        self.broadcast_state();
    }

    /// What the keyboard should be showing: the low-battery alert while it
    /// holds, otherwise the live slot's own lighting.
    ///
    /// The alert lives here and nowhere else. `current_profile` is never
    /// touched by it, and the settings file is written from
    /// `current_profile`, so alert lighting cannot reach disk by
    /// construction rather than by anyone remembering to avoid it.
    fn lighting_to_show(&self) -> Lighting {
        let slot_lighting = match self.active_slot.index() {
            Some(slot_index) => self.current_profile.slots[slot_index].clone(),
            // Off holds no lighting: write darkness rather than nothing, so
            // the keyboard matches what Aurora reports.
            None => Lighting::default(),
        };

        if !self.battery_alert_is_showing() {
            return slot_lighting;
        }

        battery_alert_lighting()
    }

    /// Whether the alert is taking the keyboard right now.
    ///
    /// A low battery is not enough on its own. With the backlight off
    /// there is nothing lit to turn red, so the alert is not showing, and
    /// reporting it as if it were would have clients draw a red keyboard
    /// next to a dark one. Off stays off: that was a choice, and an unlit
    /// keyboard cannot warn anyone anyway.
    fn battery_alert_is_showing(&self) -> bool {
        self.battery_alert_active && self.active_slot.index().is_some()
    }

    /// Read the battery and turn the alert on or off. Runs on the core
    /// thread with every other state change, so the alert needs no channel
    /// and no thread of its own.
    fn read_battery_if_due(&mut self) {
        let Some(battery_dir) = self.battery_dir.clone() else {
            return;
        };
        if Instant::now() < self.battery_read_at {
            return;
        }
        self.battery_read_at = Instant::now() + BATTERY_POLL_INTERVAL;

        // A read that fails leaves the alert where it is and tries again
        // next time: a battery that briefly cannot be read is not news.
        let Some(reading) = battery::read(&battery_dir) else {
            return;
        };

        let charge_moved = self.battery_percent != Some(reading.percent);
        self.battery_percent = Some(reading.percent);

        let alert_holds = battery_alert_holds(self.settings.battery_alert, reading);
        if alert_holds == self.battery_alert_active {
            // The alert has nothing new to say. The charge still might: a
            // client drawing the Battery gauge reads it from state, so a
            // percent that moved has to reach the client that draws it.
            if charge_moved {
                self.broadcast_state();
            }
            return;
        }

        self.battery_alert_active = alert_holds;
        let reason = if alert_holds {
            "battery low"
        } else {
            "battery recovered"
        };
        eprintln!("core: {reason} at {}%", reading.percent);
        self.apply_active_slot(reason);
    }

    /// Turn the alert on or off. Off must give the keyboard back now rather
    /// than at the next read, or the switch looks broken.
    fn set_battery_alert(&mut self, enabled: bool) -> Response {
        if self.settings.battery_alert == enabled {
            return Response::Ok;
        }

        self.settings.battery_alert = enabled;
        self.mark_changed();

        if !enabled && self.battery_alert_active {
            self.battery_alert_active = false;
            self.apply_active_slot("battery alert turned off");
            return Response::Ok;
        }

        // Turning it on must not wait out the poll interval either; the
        // next tick reads and applies.
        self.battery_read_at = Instant::now();
        self.broadcast_state();
        Response::Ok
    }

    /// One Fn+Space press. The slot advances immediately so clients see it,
    /// but the write waits for the controller's own transition to finish.
    fn advance_slot(&mut self) {
        self.active_slot = self.active_slot.next();
        self.slot_apply_at = Some(Instant::now() + SLOT_SETTLE_DELAY);
        self.custom_effect_playing = None;

        eprintln!("core: Fn+Space selected slot {}", self.active_slot);
        self.mark_changed();
        self.broadcast_state();
    }

    /// A client picked a slot. Aurora owns this number, so the pick is
    /// always honored, with or without working Fn+Space detection.
    fn select_slot(&mut self, slot: SlotSelection) -> Response {
        self.active_slot = slot;
        self.mark_changed();

        // A Fn+Space transition already in flight owns the write timing.
        // Applying now would write into the controller's transition window,
        // which is exactly what leaves the keyboard dark. Inherit the
        // deadline instead of cancelling it.
        if self.slot_apply_at.is_some() {
            eprintln!(
                "core: client selected slot {}, applying after the pending transition",
                self.active_slot
            );
            self.broadcast_state();
            return Response::Ok;
        }

        self.apply_active_slot("client selected");
        Response::Ok
    }

    /// Apply only the settled slot after an Fn+Space burst.
    fn apply_slot_if_due(&mut self) {
        let Some(apply_at) = self.slot_apply_at else {
            return;
        };
        if Instant::now() < apply_at {
            return;
        }

        self.apply_active_slot("Fn+Space settled");
    }

    fn schedule_slot_trace(&mut self) {
        if !legion_rgb_driver::trace_enabled() {
            return;
        }

        self.slot_trace_at = Some(Instant::now() + SLOT_TRACE_DELAY);
    }

    /// Diagnostic only: read the controller counter once, a fixed delay
    /// after Aurora's own slot write. It samples the counter, not the
    /// visible lighting, so a mismatch is evidence and not proof.
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

        let delay_ms = SLOT_TRACE_DELAY.as_millis();
        let active_slot = self.active_slot;
        match slot_reader.read_slot_counter() {
            Ok(counter) => eprintln!("trace: {delay_ms}ms after slot write, Aurora slot {active_slot}, controller counter {counter}"),
            Err(error) => eprintln!("trace: {delay_ms}ms after slot write, Aurora slot {active_slot}, counter unreadable ({error})"),
        }
    }

    // --- Lighting and profiles -------------------------------------------

    fn set_lighting(&mut self, slot: Option<SlotSelection>, lighting: Lighting) -> Response {
        let target_slot = match slot {
            Some(slot) => slot,
            None => self.active_slot,
        };

        let Some(slot_index) = target_slot.index() else {
            return error_response(
                ErrorKind::InvalidRequest,
                "the off position holds no lighting; select a slot first",
            );
        };

        if let Some(rejection) = validate_lighting(&lighting, self.battery_dir.is_some()) {
            return rejection;
        }

        self.current_profile.slots[slot_index] = lighting;
        self.mark_changed();

        if target_slot == self.active_slot {
            self.apply_active_slot("client edit");
        } else {
            // Editing a slot that is not live changes stored state only.
            self.broadcast_state();
        }

        Response::Ok
    }

    fn set_profile(&mut self, profile: Profile) -> Response {
        if let Some(rejection) = validate_profile(&profile, self.battery_dir.is_some()) {
            return rejection;
        }

        self.current_profile = profile;
        self.mark_changed();
        self.apply_active_slot("client set profile");
        Response::Ok
    }

    fn play_custom_effect(&mut self, effect: CustomEffect) -> Response {
        if let Some(rejection) = validate_custom_effect(&effect) {
            return rejection;
        }

        let display_name = match &effect.name {
            Some(name) => name.clone(),
            None => "Unnamed".to_string(),
        };

        if let Some(engine) = &self.engine {
            engine.play_custom_effect(effect);
        }

        self.slot_apply_at = None;
        self.custom_effect_playing = Some(display_name);
        self.broadcast_state();
        Response::Ok
    }

    /// Play a stored effect. Clients receive summaries, not bodies, so this
    /// is how they start one without mailing the body back to the daemon
    /// that already holds it.
    fn play_custom_effect_by_name(&mut self, name: &str) -> Response {
        let mut found: Option<CustomEffect> = None;
        for saved in &self.settings.effects {
            if saved.name.as_deref() == Some(name) {
                found = Some(saved.clone());
                break;
            }
        }

        match found {
            Some(effect) => self.play_custom_effect(effect),
            None => error_response(
                ErrorKind::NoSuchCustomEffect,
                &format!("no saved custom effect called '{name}'"),
            ),
        }
    }

    fn stop_custom_effect(&mut self) -> Response {
        self.apply_active_slot("custom effect stopped");
        Response::Ok
    }

    fn add_profile(&mut self, profile: Profile) -> Response {
        let Some(name) = profile.name.clone() else {
            return error_response(
                ErrorKind::InvalidRequest,
                "profile needs a name to be saved",
            );
        };
        if let Some(rejection) = validate_name(&name, "profile") {
            return rejection;
        }
        if let Some(rejection) = validate_profile(&profile, self.battery_dir.is_some()) {
            return rejection;
        }

        let mut replaced = false;
        for saved in &mut self.settings.profiles {
            if saved.name.as_deref() == Some(name.as_str()) {
                *saved = profile.clone();
                replaced = true;
                break;
            }
        }

        if !replaced {
            if self.settings.profiles.len() >= MAX_SAVED_PROFILES {
                return error_response(
                    ErrorKind::InvalidRequest,
                    &format!("cannot save more than {MAX_SAVED_PROFILES} profiles"),
                );
            }
            self.settings.profiles.push(profile);
        }

        self.mark_changed();
        self.broadcast_state();
        Response::Ok
    }

    fn delete_profile(&mut self, name: &str) -> Response {
        let position = self
            .settings
            .profiles
            .iter()
            .position(|saved| saved.name.as_deref() == Some(name));

        match position {
            Some(index) => {
                self.settings.profiles.remove(index);
                self.mark_changed();
                self.broadcast_state();
                Response::Ok
            }
            None => error_response(
                ErrorKind::NoSuchProfile,
                &format!("no saved profile called '{name}'"),
            ),
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
            // The active slot survives a profile switch: you keep looking at
            // the same position, with the new profile's lighting in it.
            Some(profile) => self.set_profile(profile),
            None => error_response(
                ErrorKind::NoSuchProfile,
                &format!("no saved profile called '{name}'"),
            ),
        }
    }

    fn cycle_profile(&mut self) -> Response {
        let profile_count = self.settings.profiles.len();
        if profile_count == 0 {
            return error_response(
                ErrorKind::NoSuchProfile,
                "no saved profiles to cycle through",
            );
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

    fn add_custom_effect(&mut self, effect: CustomEffect) -> Response {
        let Some(name) = effect.name.clone() else {
            return error_response(
                ErrorKind::InvalidRequest,
                "custom effect needs a name to be saved",
            );
        };
        if let Some(rejection) = validate_name(&name, "custom effect") {
            return rejection;
        }
        if let Some(rejection) = validate_custom_effect(&effect) {
            return rejection;
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
            if self.settings.effects.len() >= MAX_SAVED_CUSTOM_EFFECTS {
                return error_response(
                    ErrorKind::InvalidRequest,
                    &format!("cannot save more than {MAX_SAVED_CUSTOM_EFFECTS} custom effects"),
                );
            }
            self.settings.effects.push(effect);
        }

        self.mark_changed();
        self.broadcast_state();
        Response::Ok
    }

    fn delete_custom_effect(&mut self, name: &str) -> Response {
        let position = self
            .settings
            .effects
            .iter()
            .position(|saved| saved.name.as_deref() == Some(name));

        match position {
            Some(index) => {
                self.settings.effects.remove(index);
                self.mark_changed();
                self.broadcast_state();
                Response::Ok
            }
            None => error_response(
                ErrorKind::NoSuchCustomEffect,
                &format!("no saved custom effect called '{name}'"),
            ),
        }
    }

    // --- State + persistence ---------------------------------------------

    fn state_snapshot(&self) -> DaemonState {
        let mut profiles: Vec<ProfileSummary> = Vec::with_capacity(self.settings.profiles.len());
        for saved in &self.settings.profiles {
            let Some(name) = &saved.name else {
                continue;
            };
            profiles.push(ProfileSummary { name: name.clone() });
        }

        let mut custom_effects: Vec<CustomEffectSummary> =
            Vec::with_capacity(self.settings.effects.len());
        for saved in &self.settings.effects {
            let Some(name) = &saved.name else {
                continue;
            };
            custom_effects.push(CustomEffectSummary {
                name: name.clone(),
                step_count: saved.effect_steps.len(),
                should_loop: saved.should_loop,
            });
        }

        DaemonState {
            keyboard: self.keyboard_status.clone(),
            current: self.current_profile.clone(),
            active_slot: self.active_slot,
            custom_effect_playing: self.custom_effect_playing.clone(),
            profiles,
            custom_effects,
            version: env!("CARGO_PKG_VERSION").to_string(),
            settings_error: self.settings_error.clone(),
            slot_sync: self.slot_sync.clone(),
            hotkey: self.hotkey.clone(),
            screen_capture: self.screen_capture.clone(),
            battery_available: self.battery_dir.is_some(),
            battery_alert: self.settings.battery_alert,
            battery_alert_active: self.battery_alert_is_showing(),
            battery_percent: self.battery_percent,
        }
    }

    fn broadcast_state(&mut self) {
        let state = self.state_snapshot();
        let envelope = EventEnvelope {
            event: Event::StateChanged { state },
        };

        // Send to every subscriber; drop the ones whose connection is gone
        // or whose queue is full (a stuck client must not stall the core;
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
        self.settings.active_slot = self.active_slot;
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

        match self.settings.save() {
            Ok(()) => {
                self.settings_dirty = false;
                if self.settings_error.is_some() {
                    self.settings_error = None;
                    self.broadcast_state();
                }
            }
            Err(message) => {
                // The change is still unsaved, so the dirty flag stays set.
                // Push the timer out so a permanently failing save retries
                // on the debounce interval instead of on every tick.
                self.last_change_at = Instant::now();

                let is_new_failure = self.settings_error.as_deref() != Some(message.as_str());
                if is_new_failure {
                    eprintln!("core: could not save settings: {message}");
                    self.settings_error = Some(message);
                    self.broadcast_state();
                }
            }
        }
    }

    fn shutdown(mut self) {
        eprintln!("core: shutting down");

        if let Some(engine) = self.engine.take() {
            engine.shutdown();
        }

        self.settings.current_profile = self.current_profile.clone();
        self.settings.active_slot = self.active_slot;
        let save_result = self.settings.save();
        if let Err(message) = save_result {
            eprintln!("core: could not save settings on shutdown: {message}");
        }
    }
}

/// Decide the slot to start from, given the persisted selection and a
/// counter read taken before Aurora has written anything.
///
/// The two sources know different things. The controller knows whether the
/// user left the backlight off, which Aurora cannot know across a restart.
/// It does not reliably know *which* lit slot is showing, because Aurora's
/// own writes moved that number during the previous session. So: the
/// counter decides lit versus off, the persisted value decides which slot.
///
/// An unreadable or unsettled counter decides nothing, and the persisted
/// selection stands. That matters on machines where the read never works:
/// they keep their slot instead of being forced to the first one.
fn anchor_slot(
    persisted: SlotSelection,
    counter_result: Result<u8, legion_rgb_driver::error::Error>,
) -> SlotSelection {
    let counter = match counter_result {
        Ok(counter) => counter,
        Err(error) => {
            eprintln!("core: startup counter read failed ({error}); keeping slot {persisted}");
            return persisted;
        }
    };

    let Some(reported) = SlotSelection::from_counter(counter) else {
        eprintln!(
            "core: startup counter was mid-transition ({counter:#04x}); keeping slot {persisted}"
        );
        return persisted;
    };

    match (reported, persisted) {
        (SlotSelection::Off, _) => {
            eprintln!("core: startup counter says the backlight is off");
            SlotSelection::Off
        }
        // Controller says lit and so does the stored selection: trust the
        // stored one, which survived Aurora's own counter movement.
        (_, SlotSelection::First | SlotSelection::Second | SlotSelection::Third) => {
            eprintln!("core: startup counter {counter} says lit; keeping stored slot {persisted}");
            persisted
        }
        // Controller says lit, stored selection says off. The user turned
        // the backlight back on while the daemon was down, and nothing
        // records which slot they landed on.
        (_, SlotSelection::Off) => {
            eprintln!("core: startup counter {counter} says lit but the stored slot was off; starting at slot 1");
            SlotSelection::First
        }
    }
}

/// Whether the low-battery alert should be holding the keyboard.
///
/// Split out from the core so the rule is testable on its own, and stated
/// once so the switch, the poll and the tests cannot drift apart. Being on
/// AC clears the alert outright, so there is nothing for a hysteresis band
/// to protect against and none is used.
fn battery_alert_holds(enabled: bool, reading: battery::Reading) -> bool {
    enabled && reading.discharging && reading.percent <= BATTERY_ALERT_PERCENT
}

/// Every zone red at the hardware's low brightness, held steady. Built
/// fresh each time rather than stored: it is a constant dressed as a
/// value, and nothing should be able to edit it.
fn battery_alert_lighting() -> Lighting {
    let mut lighting = Lighting::solid(BATTERY_ALERT_COLOR);
    lighting.effect = Effects::Static;
    lighting.brightness = Brightness::Low;
    lighting
}

fn validate_name(name: &str, kind: &str) -> Option<Response> {
    if name.is_empty() {
        return Some(error_response(
            ErrorKind::InvalidRequest,
            &format!("{kind} name is empty"),
        ));
    }
    if name.len() > MAX_NAME_BYTES {
        return Some(error_response(
            ErrorKind::InvalidRequest,
            &format!(
                "{kind} name is {} bytes, the limit is {MAX_NAME_BYTES}",
                name.len()
            ),
        ));
    }

    None
}

fn validate_custom_effect(effect: &CustomEffect) -> Option<Response> {
    if effect.effect_steps.is_empty() {
        return Some(error_response(
            ErrorKind::InvalidRequest,
            "custom effect has no steps",
        ));
    }
    if effect.effect_steps.len() > MAX_CUSTOM_EFFECT_STEPS {
        return Some(error_response(
            ErrorKind::InvalidRequest,
            &format!(
                "custom effect has {} steps, the limit is {MAX_CUSTOM_EFFECT_STEPS}",
                effect.effect_steps.len()
            ),
        ));
    }

    None
}

fn validate_profile(profile: &Profile, battery_available: bool) -> Option<Response> {
    for lighting in &profile.slots {
        if let Some(rejection) = validate_lighting(lighting, battery_available) {
            return Some(rejection);
        }
    }

    None
}

/// Returns `Some(error response)` when the lighting is out of range, or asks
/// for hardware this machine does not have.
fn validate_lighting(lighting: &Lighting, battery_available: bool) -> Option<Response> {
    // A profile file is portable and a battery is not, so a profile written
    // on a laptop can reach a machine with nothing to gauge. Refusing it
    // says so; running it would leave a keyboard lit at a charge that does
    // not exist.
    if lighting.effect.needs_a_battery() && !battery_available {
        return Some(error_response(
            ErrorKind::InvalidRequest,
            &format!(
                "the {} effect needs a battery, and this machine has none",
                lighting.effect
            ),
        ));
    }

    if !SOFTWARE_SPEED_RANGE.contains(&lighting.speed) {
        return Some(error_response(
            ErrorKind::InvalidRequest,
            &format!(
                "speed {} outside {:?}",
                lighting.speed, SOFTWARE_SPEED_RANGE
            ),
        ));
    }

    if let aurora_protocol::effects::Effects::AmbientLight {
        fps,
        saturation_boost,
    } = lighting.effect
    {
        if !(1..=60).contains(&fps) {
            return Some(error_response(
                ErrorKind::InvalidRequest,
                &format!("ambient fps {fps} outside 1..=60"),
            ));
        }
        if !(0.0..=1.0).contains(&saturation_boost) {
            return Some(error_response(
                ErrorKind::InvalidRequest,
                "ambient saturation boost outside 0.0..=1.0",
            ));
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
    use aurora_protocol::ipc::SlotSelection;
    use legion_rgb_driver::error::{Error, RangeError, RangeErrorKind};
    use strum::IntoEnumIterator;

    use super::{
        anchor_slot, battery, battery_alert_holds, validate_lighting, validate_profile, Effects,
        Lighting, Profile, BATTERY_ALERT_PERCENT,
    };

    fn reading(percent: u8, discharging: bool) -> battery::Reading {
        battery::Reading {
            percent,
            discharging,
        }
    }

    #[test]
    fn the_alert_holds_at_and_below_the_threshold_while_discharging() {
        assert!(battery_alert_holds(
            true,
            reading(BATTERY_ALERT_PERCENT, true)
        ));
        assert!(battery_alert_holds(true, reading(1, true)));
        assert!(battery_alert_holds(true, reading(0, true)));
    }

    #[test]
    fn the_alert_lets_go_one_percent_above_the_threshold() {
        assert!(!battery_alert_holds(
            true,
            reading(BATTERY_ALERT_PERCENT + 1, true)
        ));
    }

    /// Conservation mode reports `Not charging` on AC, which reaches here
    /// as `discharging: false`. Plugged in is plugged in, however flat.
    #[test]
    fn being_on_ac_clears_the_alert_at_any_charge() {
        assert!(!battery_alert_holds(true, reading(1, false)));
        assert!(!battery_alert_holds(true, reading(0, false)));
    }

    #[test]
    fn the_switch_turns_the_alert_off_outright() {
        assert!(!battery_alert_holds(false, reading(1, true)));
    }

    /// A profile file is portable and a battery is not, so this is the
    /// path a laptop profile takes when it is opened on a desktop.
    #[test]
    fn the_battery_effect_is_refused_where_there_is_no_battery() {
        let lighting = Lighting {
            effect: Effects::Battery,
            ..Default::default()
        };

        assert!(validate_lighting(&lighting, false).is_some());
        assert!(validate_lighting(&lighting, true).is_none());
    }

    /// One slot is enough to refuse the whole profile: applying it would
    /// start the gauge as soon as that slot came round.
    #[test]
    fn one_battery_slot_refuses_the_whole_profile() {
        let mut profile = Profile::default();
        profile.slots[2].effect = Effects::Battery;

        assert!(validate_profile(&profile, false).is_some());
        assert!(validate_profile(&profile, true).is_none());
    }

    /// The rejection must be about the battery and nothing else; every
    /// other effect has to survive a machine without one.
    #[test]
    fn every_other_effect_survives_a_machine_without_a_battery() {
        for effect in Effects::iter() {
            if effect.needs_a_battery() {
                continue;
            }

            // The iterator yields zeroed fields, and ambient's are checked
            // separately; give it the defaults a client would send.
            let effect = match effect {
                Effects::AmbientLight { .. } => Effects::AmbientLight {
                    fps: 30,
                    saturation_boost: 0.0,
                },
                other => other,
            };
            let lighting = Lighting {
                effect,
                ..Default::default()
            };

            assert!(
                validate_lighting(&lighting, false).is_none(),
                "{effect} was refused on a machine with no battery"
            );
        }
    }

    #[test]
    fn off_on_the_counter_wins_over_any_stored_slot() {
        // The user pressed Fn+Space to off while the daemon was down. Only
        // the controller knows that.
        let anchored = anchor_slot(SlotSelection::Second, Ok(4));
        assert_eq!(anchored, SlotSelection::Off);
    }

    #[test]
    fn a_lit_counter_keeps_the_stored_slot() {
        // The counter says lit but its number is unreliable, because
        // Aurora's own writes moved it last session. The stored slot wins.
        let anchored = anchor_slot(SlotSelection::Third, Ok(1));
        assert_eq!(anchored, SlotSelection::Third);
    }

    #[test]
    fn a_lit_counter_recovers_from_a_stored_off() {
        let anchored = anchor_slot(SlotSelection::Off, Ok(2));
        assert_eq!(anchored, SlotSelection::First);
    }

    /// The regression that forced every machine with an unreadable counter
    /// onto slot 1, overwriting whatever the user last chose.
    #[test]
    fn an_unreadable_counter_keeps_the_stored_slot() {
        let read_error = Error::RangeError(RangeError {
            kind: RangeErrorKind::Slot,
        });
        let anchored = anchor_slot(SlotSelection::Third, Err(read_error));
        assert_eq!(anchored, SlotSelection::Third);
    }

    #[test]
    fn a_mid_transition_counter_keeps_the_stored_slot() {
        // Observed live: 0 persisted for over 20 seconds on a 2023 Pro.
        assert_eq!(
            anchor_slot(SlotSelection::Second, Ok(0)),
            SlotSelection::Second
        );
        assert_eq!(anchor_slot(SlotSelection::Off, Ok(9)), SlotSelection::Off);
    }
}
