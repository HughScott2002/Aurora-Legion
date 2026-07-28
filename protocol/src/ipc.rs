//! IPC schema spoken between the daemon and its clients (GUI, CLI).
//!
//! Transport: JSON-lines over a unix domain socket. Every line is one JSON
//! object. Clients send [`RequestEnvelope`] lines; the daemon answers with
//! [`ResponseEnvelope`] lines and, after a [`Request::Subscribe`], also
//! pushes [`EventEnvelope`] lines on the same connection.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    custom_effect::CustomEffect,
    profile::{Lighting, Profile, SLOT_COUNT},
};

/// Upper bound for a single JSON line. Custom effects with many steps are the
/// largest payload; one mebibyte gives them plenty of headroom while keeping
/// a misbehaving peer from ballooning daemon memory.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Version of the IPC schema in this file. Bump on any change that an
/// existing client would misread: renamed fields, removed variants, changed
/// semantics. Additive changes (new requests, new optional fields) do not
/// bump it; unknown variants already fail parsing loudly.
///
/// Clients send [`Request::Hello`] first and compare the daemon's answer;
/// see `docs/protocol.md` for the negotiation rules.
/// Version 2 moved the lighting configuration from `Profile` into
/// `Profile::slots`, so a v1 client would misread every profile it receives.
pub const PROTOCOL_VERSION: u32 = 2;

/// Upper bound on saved profiles and custom effects. Without these the
/// state broadcast grows without limit and eventually exceeds
/// [`MAX_LINE_BYTES`], which disconnects every client on every broadcast.
pub const MAX_SAVED_PROFILES: usize = 128;
pub const MAX_SAVED_CUSTOM_EFFECTS: usize = 128;

/// Upper bound on custom effect length, so one bad file cannot balloon the
/// settings file.
pub const MAX_CUSTOM_EFFECT_STEPS: usize = 4096;

pub const SOCKET_FILE_NAME: &str = "aurora.sock";

/// Which Fn+Space position is live. The controller cycles three lit slots
/// and an off position; this enum is closed so an out-of-range slot cannot
/// be represented, let alone sent.
///
/// After the daemon acquires the keyboard this is Aurora's own number, not
/// a live controller reading: Aurora's lighting writes move the controller's
/// counter without raising the Fn+Space event, so the counter is trusted
/// exactly once. See `docs/explanation/fn-space-sync.md`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotSelection {
    First,
    Second,
    Third,
    /// Backlight off. Holds no lighting, and rejects edits.
    Off,
}

/// The lit slots, in the order Fn+Space walks them.
pub const LIT_SLOTS: [SlotSelection; SLOT_COUNT] = [SlotSelection::First, SlotSelection::Second, SlotSelection::Third];

/// Counter value the controller reports while the backlight is off.
const SLOT_COUNTER_OFF: u8 = 4;

impl SlotSelection {
    /// Index into [`Profile::slots`], or `None` for off.
    pub fn index(self) -> Option<usize> {
        match self {
            SlotSelection::First => Some(0),
            SlotSelection::Second => Some(1),
            SlotSelection::Third => Some(2),
            SlotSelection::Off => None,
        }
    }

    /// The number a user sees, or `None` for off.
    pub fn number(self) -> Option<u8> {
        let index = self.index()?;
        Some(index as u8 + 1)
    }

    /// The next position Fn+Space moves to.
    pub fn next(self) -> Self {
        match self {
            SlotSelection::First => SlotSelection::Second,
            SlotSelection::Second => SlotSelection::Third,
            SlotSelection::Third => SlotSelection::Off,
            SlotSelection::Off => SlotSelection::First,
        }
    }

    /// Read a controller counter byte. `None` for any value the controller
    /// reports mid-transition, which callers must treat as "not settled"
    /// rather than as a slot.
    pub fn from_counter(value: u8) -> Option<Self> {
        match value {
            1 => Some(SlotSelection::First),
            2 => Some(SlotSelection::Second),
            3 => Some(SlotSelection::Third),
            SLOT_COUNTER_OFF => Some(SlotSelection::Off),
            _ => None,
        }
    }
}

impl std::fmt::Display for SlotSelection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.number() {
            Some(number) => write!(f, "{number}"),
            None => write!(f, "off"),
        }
    }
}

/// Path of the daemon socket: `$XDG_RUNTIME_DIR/aurora.sock`, with a
/// `/tmp` fallback for sessions without a runtime dir.
pub fn socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR");

    match runtime_dir {
        Ok(dir) if !dir.is_empty() => {
            let mut path = PathBuf::from(dir);
            path.push(SOCKET_FILE_NAME);
            path
        }
        _ => {
            let mut path = PathBuf::from("/tmp");
            path.push(SOCKET_FILE_NAME);
            path
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RequestEnvelope {
    /// Client-chosen id echoed back in the matching [`ResponseEnvelope`].
    pub id: u64,
    pub req: Request,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum Request {
    /// Version handshake. Send first on a new connection; the daemon
    /// answers [`Response::Hello`]. A daemon too old to know this request
    /// answers `Error { kind: InvalidRequest }` instead, which clients
    /// should report as a version mismatch, not a protocol failure.
    Hello { protocol_version: u32 },
    /// Return the full daemon state.
    GetState,
    /// Make `profile` the live profile, all slots at once, and apply the
    /// active slot. Stops any playing custom effect.
    SetProfile { profile: Profile },
    /// Replace one slot's lighting in the live profile. `slot` of `None`
    /// targets whichever slot is active. Targeting [`SlotSelection::Off`]
    /// is rejected: off holds no lighting.
    SetLighting { slot: Option<SlotSelection>, lighting: Lighting },
    /// Make `slot` the live position and apply it. This moves Aurora's own
    /// slot number, the same one Fn+Space moves; it cannot drive the
    /// controller's counter, because no such command exists on this
    /// hardware.
    SelectSlot { slot: SlotSelection },
    /// Start playing a custom effect until stopped or replaced.
    PlayCustomEffect { effect: CustomEffect },
    /// Play a saved custom effect by name. Clients that already received a
    /// [`CustomEffectSummary`] use this instead of sending the body back to
    /// the daemon that stored it.
    PlayCustomEffectByName { name: String },
    /// Stop the playing custom effect and re-apply the active slot.
    StopCustomEffect,
    /// Save a named profile. Overwrites a saved profile with the same name.
    AddProfile { profile: Profile },
    DeleteProfile { name: String },
    /// Make the saved profile called `name` the live profile.
    SwitchProfile { name: String },
    /// Advance to the next saved profile (wraps around).
    CycleProfile,
    /// Save a named custom effect. Overwrites one with the same name.
    AddCustomEffect { effect: CustomEffect },
    DeleteCustomEffect { name: String },
    /// Receive a [`Event::StateChanged`] line on this connection whenever
    /// the daemon state changes.
    Subscribe,
    /// Ask the daemon to exit cleanly.
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ResponseEnvelope {
    /// Mirrors the `id` of the request this response answers.
    pub id: u64,
    pub resp: Response,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum Response {
    /// Answer to [`Request::Hello`]. `protocol_version` is the daemon's
    /// [`PROTOCOL_VERSION`]; `daemon_version` is its package version.
    /// The daemon answers regardless of the client's version (and logs a
    /// warning on mismatch); enforcement is the client's call.
    Hello { protocol_version: u32, daemon_version: String },
    Ok,
    State { state: DaemonState },
    Error { kind: ErrorKind, message: String },
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    KeyboardNotFound,
    PermissionDenied,
    NoSuchProfile,
    NoSuchCustomEffect,
    InvalidRequest,
    Internal,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct EventEnvelope {
    pub event: Event,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum Event {
    /// Full state snapshot. The state is small, so clients replace rather
    /// than patch; there is no incremental sync to get wrong.
    StateChanged { state: DaemonState },
}

/// One of the two line shapes the daemon writes. Clients deserialize into
/// this and match; `untagged` works because `resp` and `event` are distinct
/// field names.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum ServerMessage {
    Response(ResponseEnvelope),
    Event(EventEnvelope),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type")]
pub enum KeyboardStatus {
    /// Keyboard acquired; effects are being applied.
    Connected,
    /// No keyboard found yet; the daemon retries with backoff.
    Searching,
    /// A keyboard exists but the daemon may not open it (udev rule missing).
    PermissionDenied { message: String },
    /// Any other acquisition or runtime device failure.
    Error { message: String },
}

/// A saved profile as clients see it in a state broadcast. Bodies stay in
/// the daemon: a broadcast carrying every profile and every custom effect
/// body grows past [`MAX_LINE_BYTES`], and clients only render names.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProfileSummary {
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CustomEffectSummary {
    pub name: String,
    pub step_count: usize,
    pub should_loop: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DaemonState {
    pub keyboard: KeyboardStatus,
    /// The live profile, all slots. The keyboard shows
    /// `current.slots[active_slot]` unless a custom effect plays or
    /// `active_slot` is off.
    pub current: Profile,
    /// Which Fn+Space position is live.
    pub active_slot: SlotSelection,
    /// Name of the playing custom effect, if any.
    pub custom_effect_playing: Option<String>,
    pub profiles: Vec<ProfileSummary>,
    pub custom_effects: Vec<CustomEffectSummary>,
    /// Daemon package version, so clients can spot mismatches.
    pub version: String,
    /// Why the last settings write failed, if it did. Lighting keeps
    /// working when persistence does not, so this is reported rather than
    /// fatal.
    pub settings_error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_state() -> DaemonState {
        DaemonState {
            keyboard: KeyboardStatus::PermissionDenied {
                message: "hidraw: permission denied".to_string(),
            },
            current: Profile::default(),
            active_slot: SlotSelection::Second,
            custom_effect_playing: Some("pulse".to_string()),
            profiles: vec![ProfileSummary { name: "gaming".to_string() }],
            custom_effects: vec![CustomEffectSummary {
                name: "pulse".to_string(),
                step_count: 12,
                should_loop: true,
            }],
            version: "0.24.0".to_string(),
            settings_error: None,
        }
    }

    #[test]
    fn request_round_trips() {
        let request = RequestEnvelope {
            id: 7,
            req: Request::SetProfile { profile: Profile::default() },
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: RequestEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(request, parsed);
    }

    #[test]
    fn response_round_trips() {
        let response = ResponseEnvelope {
            id: 7,
            resp: Response::State { state: sample_state() },
        };

        let json = serde_json::to_string(&response).unwrap();
        let parsed: ResponseEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(response, parsed);
    }

    #[test]
    fn server_message_demuxes_responses_and_events() {
        let response_json = serde_json::to_string(&ResponseEnvelope { id: 1, resp: Response::Ok }).unwrap();
        let event_json = serde_json::to_string(&EventEnvelope {
            event: Event::StateChanged { state: sample_state() },
        })
        .unwrap();

        let parsed_response: ServerMessage = serde_json::from_str(&response_json).unwrap();
        let parsed_event: ServerMessage = serde_json::from_str(&event_json).unwrap();

        assert!(matches!(parsed_response, ServerMessage::Response(_)));
        assert!(matches!(parsed_event, ServerMessage::Event(_)));
    }

    #[test]
    fn slot_selection_cycles_three_lit_then_off() {
        let mut slot = SlotSelection::First;
        let expected = [SlotSelection::Second, SlotSelection::Third, SlotSelection::Off, SlotSelection::First];

        for expected_slot in expected {
            slot = slot.next();
            assert_eq!(slot, expected_slot);
        }
    }

    #[test]
    fn slot_selection_maps_settled_counter_values_only() {
        assert_eq!(SlotSelection::from_counter(1), Some(SlotSelection::First));
        assert_eq!(SlotSelection::from_counter(3), Some(SlotSelection::Third));
        assert_eq!(SlotSelection::from_counter(4), Some(SlotSelection::Off));

        // Values the controller reports mid-transition. Observed live: 0
        // persisted for over 20 seconds on a 2023 Pro.
        assert_eq!(SlotSelection::from_counter(0), None);
        assert_eq!(SlotSelection::from_counter(5), None);
        assert_eq!(SlotSelection::from_counter(255), None);
    }

    #[test]
    fn slot_index_and_number_agree_with_lit_slots() {
        for (position, slot) in LIT_SLOTS.iter().enumerate() {
            assert_eq!(slot.index(), Some(position));
            assert_eq!(slot.number(), Some(position as u8 + 1));
        }

        assert_eq!(SlotSelection::Off.index(), None);
        assert_eq!(SlotSelection::Off.number(), None);
    }

    #[test]
    fn hello_round_trips() {
        let request = RequestEnvelope {
            id: 1,
            req: Request::Hello { protocol_version: PROTOCOL_VERSION },
        };
        let response = ResponseEnvelope {
            id: 1,
            resp: Response::Hello {
                protocol_version: PROTOCOL_VERSION,
                daemon_version: "0.23.0".to_string(),
            },
        };

        let request_json = serde_json::to_string(&request).unwrap();
        let response_json = serde_json::to_string(&response).unwrap();

        let parsed_request: RequestEnvelope = serde_json::from_str(&request_json).unwrap();
        let parsed_response: ResponseEnvelope = serde_json::from_str(&response_json).unwrap();

        assert_eq!(request, parsed_request);
        assert_eq!(response, parsed_response);
    }

    /// Clients that never send Hello (all pre-handshake clients) must keep
    /// working; the handshake is opt-in.
    #[test]
    fn requests_without_hello_still_parse() {
        let json = r#"{"id":2,"req":{"type":"GetState"}}"#;
        let parsed: RequestEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.req, Request::GetState);
    }

    #[test]
    fn socket_path_uses_runtime_dir() {
        let path = socket_path();
        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(file_name, SOCKET_FILE_NAME);
    }

    /// A v1 client's Hello must still PARSE, so the daemon can answer with
    /// a version mismatch instead of an unhelpful parse error.
    #[test]
    fn v1_hello_still_parses_so_mismatch_is_reportable() {
        let json = r#"{"id":1,"req":{"type":"Hello","protocol_version":1}}"#;
        let parsed: RequestEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.req, Request::Hello { protocol_version: 1 });
        assert_ne!(PROTOCOL_VERSION, 1, "v2 is the current version");
    }
}
