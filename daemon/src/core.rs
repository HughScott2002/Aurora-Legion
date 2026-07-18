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

use crate::{
    engine::{EffectManager, StopSignals, SOFTWARE_SPEED_RANGE},
    keyboard::{self, AcquireOutcome},
    settings::Settings,
};

/// How long the core waits for a command before running its housekeeping
/// tick (keyboard reacquisition, debounced settings save, device errors).
const TICK_MS: u64 = 250;

/// The live profile is saved this long after the last change, so a GUI
/// slider drag does not write the file on every wiggle.
const SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

/// Upper bound on custom effect length, so one bad file cannot balloon the
/// settings file and every state broadcast with it.
const MAX_CUSTOM_EFFECT_STEPS: usize = 4096;

/// Commands the core accepts. Keep this the only way to mutate daemon state.
pub enum Command {
    Ipc {
        envelope_id: u64,
        request: Request,
        out_tx: Sender<Outbound>,
    },
    CycleProfile,
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

    subscribers: Vec<Sender<Outbound>>,

    settings_dirty: bool,
    last_change_at: Instant,

    acquire_attempt_count: u32,
    next_acquire_at: Instant,

    shutdown_requested: bool,
}

pub fn run(command_rx: &Receiver<Command>, shutdown_flag: &Arc<AtomicBool>) {
    let settings = Settings::load_or_migrate();
    let current_profile = settings.current_profile.clone();

    let mut core = Core {
        settings,
        current_profile,
        custom_effect_playing: None,
        keyboard_status: KeyboardStatus::Searching,
        engine: None,
        stop_signals: StopSignals::new(),
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

        match command_rx.recv_timeout(Duration::from_millis(TICK_MS)) {
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
                eprintln!("core: keyboard acquired, applying current profile");
                let engine = EffectManager::new(*keyboard, self.stop_signals.clone());
                engine.set_profile(self.current_profile.clone());
                self.engine = Some(engine);
                self.keyboard_status = KeyboardStatus::Connected;
                self.custom_effect_playing = None;
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
        }
    }

    fn handle_request(&mut self, request: Request, out_tx: &Sender<Outbound>) -> Response {
        match request {
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

    fn set_profile(&mut self, profile: Profile) -> Response {
        if let Some(rejection) = validate_profile(&profile) {
            return rejection;
        }

        self.current_profile = profile.clone();
        self.custom_effect_playing = None;
        if let Some(engine) = &self.engine {
            engine.set_profile(profile);
        }

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

    // --- State + persistence ---------------------------------------------

    fn state_snapshot(&self) -> DaemonState {
        DaemonState {
            keyboard: self.keyboard_status.clone(),
            current: self.current_profile.clone(),
            custom_effect_playing: self.custom_effect_playing.clone(),
            profiles: self.settings.profiles.clone(),
            custom_effects: self.settings.effects.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
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
