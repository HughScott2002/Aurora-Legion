//! Plain-language problems. Backend diagnostics belong in Details only.

use std::time::Duration;

use aurora_protocol::ipc::{ErrorKind, KeyboardStatus};

pub const RECOVERY_NOTICE_DELAY: Duration = Duration::from_secs(2);
pub const RECOVERY_HELP_DELAY: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Help {
    Recovering,
    Connection,
    Setup,
    Update,
    EffectFile,
    Details,
}

#[derive(Debug, Clone)]
pub struct Problem {
    pub title: &'static str,
    pub description: &'static str,
    pub details: String,
    pub help: Help,
}

impl Problem {
    pub fn button_label(&self) -> Option<&'static str> {
        match self.help {
            Help::Recovering => None,
            Help::Connection => Some("Connection Help"),
            Help::Setup => Some("Show Setup Steps"),
            Help::Update => Some("Show Update Steps"),
            Help::EffectFile => Some("Choose Another File"),
            Help::Details => Some("Details"),
        }
    }

    pub fn connection(details: String) -> Self {
        Self {
            title: "The lighting service is unavailable.",
            description: "Aurora is trying to reconnect. If this continues, try restarting the \
                          lighting service from Settings.",
            details,
            help: Help::Connection,
        }
    }

    pub fn incompatible(details: String) -> Self {
        Self {
            title: "Some installed parts of Aurora do not match.",
            description: "Update Aurora, then reopen the app. The app and lighting service \
                          need to come from the same installation.",
            details,
            help: Help::Update,
        }
    }

    pub fn file(details: String) -> Self {
        Self {
            title: "This file could not be opened as an Aurora effect.",
            description: "Choose an Aurora effect file and try again.",
            details,
            help: Help::EffectFile,
        }
    }
}

pub fn keyboard_problem(status: &KeyboardStatus, seen: bool, elapsed: Duration) -> Option<Problem> {
    let details = match status {
        KeyboardStatus::Connected => return None,
        KeyboardStatus::PermissionDenied { message } => {
            return Some(access_problem(message.clone()))
        }
        KeyboardStatus::Error { message } => message.clone(),
        _ => String::new(),
    };
    if !seen {
        return Some(Problem {
            title: "Aurora could not connect to a supported keyboard.",
            description: "Check that your laptop is supported and keyboard access is set up. \
                          Aurora will connect automatically when the keyboard becomes \
                          available.",
            details,
            help: Help::Connection,
        });
    }
    if matches!(status, KeyboardStatus::Lost) || elapsed >= RECOVERY_HELP_DELAY {
        return Some(Problem {
            title: "Your keyboard is still unavailable.",
            description: "Aurora will reconnect when it returns. You can keep editing your \
                          profiles. If this continues, open the connection help.",
            details,
            help: Help::Connection,
        });
    }
    if elapsed < RECOVERY_NOTICE_DELAY {
        return None;
    }
    Some(Problem {
        title: "Reconnecting to your keyboard…",
        description: "Aurora is trying to restore the connection automatically.",
        details,
        help: Help::Recovering,
    })
}

fn access_problem(details: String) -> Problem {
    Problem {
        title: "Aurora needs permission to control your keyboard.",
        description: "Complete the keyboard access setup for your system, then sign out and \
                      back in. Aurora will reconnect automatically.",
        details,
        help: Help::Setup,
    }
}

pub fn request_problem(kind: ErrorKind, details: String) -> Problem {
    if kind == ErrorKind::PermissionDenied {
        return access_problem(details);
    }
    if kind == ErrorKind::KeyboardNotFound {
        return Problem {
            title: "Your keyboard is unavailable.",
            description: "Aurora will reconnect automatically when the keyboard returns. \
                          If this continues, open the connection help.",
            details,
            help: Help::Connection,
        };
    }
    Problem {
        title: "That change could not be applied.",
        description: "Check your selection and try again.",
        details,
        help: Help::Details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brief_disconnects_stay_quiet_but_longer_ones_offer_help() {
        assert!(keyboard_problem(&KeyboardStatus::Searching, true, Duration::ZERO).is_none());
        let recovering = keyboard_problem(&KeyboardStatus::Searching, true, RECOVERY_NOTICE_DELAY);
        assert!(recovering.is_some());
        let persistent = keyboard_problem(&KeyboardStatus::Searching, true, RECOVERY_HELP_DELAY)
            .expect("persistent loss needs a message");
        assert_eq!(persistent.help, Help::Connection);
    }

    #[test]
    fn access_errors_offer_setup_without_exposing_backend_text() {
        let raw = "libusb: Permission denied (os error 13)";
        let status = KeyboardStatus::PermissionDenied {
            message: raw.to_string(),
        };
        let problem =
            keyboard_problem(&status, true, Duration::ZERO).expect("access errors need setup help");
        assert_eq!(problem.help, Help::Setup);
        assert!(!problem.title.contains(raw));
        assert_eq!(problem.details, raw);
    }

    #[test]
    fn a_connected_keyboard_has_no_problem() {
        assert!(keyboard_problem(&KeyboardStatus::Connected, true, RECOVERY_HELP_DELAY).is_none());
    }

    #[test]
    fn request_permission_errors_offer_the_same_setup_help() {
        let problem = request_problem(ErrorKind::PermissionDenied, "permission denied".to_string());
        assert_eq!(problem.help, Help::Setup);
    }

    #[test]
    fn missing_keyboard_errors_do_not_blame_the_running_service() {
        let problem = request_problem(ErrorKind::KeyboardNotFound, "not found".to_string());
        assert_eq!(problem.title, "Your keyboard is unavailable.");
        assert_eq!(problem.help, Help::Connection);
    }
}
