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
pub const ACQUIRE_BACKOFF: [Duration; 4] = [Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(5), Duration::from_secs(10)];

pub fn backoff_delay(attempt_count: u32) -> Duration {
    let last_index = ACQUIRE_BACKOFF.len() - 1;
    let index = (attempt_count as usize).min(last_index);
    ACQUIRE_BACKOFF[index]
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
            let message = hid_error.to_string();
            // hidapi reports EACCES as a plain message; string matching is
            // the only classification hook it gives us.
            let lower = message.to_lowercase();
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
