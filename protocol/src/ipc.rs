//! IPC schema spoken between the daemon and its clients (GUI, CLI).
//!
//! Transport: JSON-lines over a unix domain socket. Every line is one JSON
//! object. Clients send [`RequestEnvelope`] lines; the daemon answers with
//! [`ResponseEnvelope`] lines and, after a [`Request::Subscribe`], also
//! pushes [`EventEnvelope`] lines on the same connection.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{custom_effect::CustomEffect, profile::Profile};

/// Upper bound for a single JSON line. Custom effects with many steps are the
/// largest payload; one mebibyte gives them plenty of headroom while keeping
/// a misbehaving peer from ballooning daemon memory.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

pub const SOCKET_FILE_NAME: &str = "legion-kb-rgb.sock";

/// Path of the daemon socket: `$XDG_RUNTIME_DIR/legion-kb-rgb.sock`, with a
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
    /// Return the full daemon state.
    GetState,
    /// Make `profile` the live profile and apply it to the keyboard.
    /// Stops any playing custom effect.
    SetProfile { profile: Profile },
    /// Start playing a custom effect until stopped or replaced.
    PlayCustomEffect { effect: CustomEffect },
    /// Stop the playing custom effect and re-apply the current profile.
    StopCustomEffect,
    ListProfiles,
    /// Save a named profile. Overwrites a saved profile with the same name.
    AddProfile { profile: Profile },
    DeleteProfile { name: String },
    /// Make the saved profile called `name` the live profile.
    SwitchProfile { name: String },
    /// Advance to the next saved profile (wraps around).
    CycleProfile,
    ListCustomEffects,
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
    Ok,
    State { state: DaemonState },
    Profiles { profiles: Vec<Profile> },
    CustomEffects { effects: Vec<CustomEffect> },
    Error { kind: ErrorKind, message: String },
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    KeyboardNotFound,
    PermissionDenied,
    NoSuchProfile,
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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DaemonState {
    pub keyboard: KeyboardStatus,
    /// The live profile (what the keyboard shows unless a custom effect plays).
    pub current: Profile,
    /// Name of the playing custom effect, if any.
    pub custom_effect_playing: Option<String>,
    pub profiles: Vec<Profile>,
    pub custom_effects: Vec<CustomEffect>,
    /// Daemon package version, so clients can spot mismatches.
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effects;

    fn sample_state() -> DaemonState {
        DaemonState {
            keyboard: KeyboardStatus::PermissionDenied {
                message: "hidraw: permission denied".to_string(),
            },
            current: Profile::default(),
            custom_effect_playing: Some("pulse".to_string()),
            profiles: vec![Profile {
                name: Some("gaming".to_string()),
                effect: Effects::AmbientLight { fps: 30, saturation_boost: 0.5 },
                ..Profile::default()
            }],
            custom_effects: Vec::new(),
            version: "0.21.0".to_string(),
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
    fn socket_path_uses_runtime_dir() {
        let path = socket_path();
        let file_name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(file_name, SOCKET_FILE_NAME);
    }

    /// The old app serialized settings with these exact field names; the
    /// daemon must keep parsing them for migration.
    #[test]
    fn legacy_profile_json_still_parses() {
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

        let parsed: Profile = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.name.as_deref(), Some("old"));
        assert_eq!(parsed.effect, Effects::Breath);
        assert_eq!(parsed.speed, 3);
        assert!(!parsed.rgb_zones[2].enabled);
    }
}
