//! Keyboard acquisition with retry and error classification.
//!
//! The daemon may start before the USB bus has enumerated (boot) or before
//! the udev rule exists (fresh install). Acquisition failures are therefore
//! states to report, not reasons to exit.

use std::time::Duration;

use aurora_protocol::ipc::KeyboardStatus;
use legion_rgb_driver::Keyboard;

use crate::engine::StopSignals;

/// Backoff schedule between acquisition attempts. The last entry repeats
/// forever, so a daemon started with the lid closed or the rule missing
/// keeps trying every ten seconds without spamming.
pub const ACQUIRE_BACKOFF: [Duration; 4] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
];

pub fn backoff_delay(attempt_count: u32) -> Duration {
    let last_index = ACQUIRE_BACKOFF.len() - 1;
    let index = (attempt_count as usize).min(last_index);
    ACQUIRE_BACKOFF[index]
}

/// How long a keyboard that stopped answering is still reported as a
/// reconnect in progress. Past this the daemon says the controller is gone
/// instead of promising a return. Suspend and resume take the device away
/// for a few seconds, so the wait has to outlast that; thirty seconds is
/// three attempts at the ten second ceiling of [`ACQUIRE_BACKOFF`].
pub const LOST_GRACE: Duration = Duration::from_secs(30);

/// hidapi's libusb backend does not implement `hid_error`, so every failure
/// through it carries this placeholder where the reason belongs.
const HIDAPI_MISSING_REASON: &str = "hid_error is not implemented yet";

/// What Aurora says happened, for both the log and the protocol. hidapi's
/// placeholder is not an error message, and sending it on told users to
/// debug a function name instead of their keyboard. Matching the string is
/// the only hook the backend gives us.
pub fn readable_error(text: String) -> String {
    if text.contains(HIDAPI_MISSING_REASON) {
        return "the keyboard did not answer, and the HID backend does not report why".to_string();
    }

    text
}

/// A keyboard that was working and stopped answering reads as `Searching`,
/// which promises a reconnect that cannot happen once the controller has
/// left the bus. Past [`LOST_GRACE`], say it is gone instead. Only the
/// empty-bus case is rewritten: a device that is present but will not open
/// has a status of its own, and that one the user can act on.
///
/// `lost_for` is how long ago the keyboard stopped answering, or `None` on
/// a machine where one has not been acquired this session.
pub fn status_after_loss(status: KeyboardStatus, lost_for: Option<Duration>) -> KeyboardStatus {
    if status != KeyboardStatus::Searching {
        return status;
    }

    match lost_for {
        Some(elapsed) if elapsed >= LOST_GRACE => KeyboardStatus::Lost,
        _ => status,
    }
}

pub enum AcquireOutcome {
    Acquired(Box<Keyboard>),
    Failed(KeyboardStatus),
}

pub fn try_acquire(stop_signals: &StopSignals) -> AcquireOutcome {
    let acquire_result = legion_rgb_driver::get_keyboard(stop_signals.keyboard_stop_signal.clone());

    match acquire_result {
        Ok(keyboard) => AcquireOutcome::Acquired(Box::new(keyboard)),
        Err(error) => AcquireOutcome::Failed(classify_error(&error)),
    }
}

fn classify_error(error: &legion_rgb_driver::error::Error) -> KeyboardStatus {
    use legion_rgb_driver::error::Error;

    match error {
        Error::DeviceNotFound => KeyboardStatus::Searching,
        Error::HidError(hid_error) => {
            let raw = hid_error.to_string();
            // hidapi reports EACCES as a plain message; string matching is
            // the only classification hook it gives us.
            let lower = raw.to_lowercase();
            let message = readable_error(raw);
            if lower.contains("permission denied") || lower.contains("not permitted") {
                KeyboardStatus::PermissionDenied { message }
            } else {
                KeyboardStatus::Error { message }
            }
        }
        Error::RangeError(range_error) => KeyboardStatus::Error {
            message: range_error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hidapi_placeholder_never_reaches_a_user() {
        let placeholder = "hidapi error: hid_error is not implemented yet".to_string();
        let described = readable_error(placeholder);

        assert!(!described.contains("hid_error"));
        assert!(described.contains("did not answer"));
    }

    #[test]
    fn a_real_reason_is_passed_through_untouched() {
        let real = "Permission denied (os error 13)".to_string();

        assert_eq!(readable_error(real.clone()), real);
    }

    #[test]
    fn a_keyboard_that_was_never_here_stays_searching() {
        let settled = status_after_loss(KeyboardStatus::Searching, None);

        assert_eq!(settled, KeyboardStatus::Searching);
    }

    #[test]
    fn a_keyboard_gone_a_moment_is_still_reconnecting() {
        let settled = status_after_loss(KeyboardStatus::Searching, Some(Duration::from_secs(2)));

        assert_eq!(settled, KeyboardStatus::Searching);
    }

    #[test]
    fn a_keyboard_gone_past_the_grace_is_reported_lost() {
        let settled = status_after_loss(KeyboardStatus::Searching, Some(LOST_GRACE));

        assert_eq!(settled, KeyboardStatus::Lost);
    }

    #[test]
    fn a_device_that_will_not_open_keeps_its_own_status() {
        let denied = KeyboardStatus::PermissionDenied {
            message: "Permission denied".to_string(),
        };
        let settled = status_after_loss(denied.clone(), Some(LOST_GRACE * 2));

        assert_eq!(settled, denied);
    }
}
