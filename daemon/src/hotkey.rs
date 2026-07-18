//! Meta+RAlt profile cycling, ported from the old GUI's polling thread.
//!
//! device_query talks to X11 (XWayland under GNOME), so this works only
//! while a graphical session exists — which the systemd unit guarantees by
//! binding to `graphical-session.target`. If the display connection cannot
//! be opened the hotkey is disabled and the daemon carries on;
//! `legion-kb cycle-profile` bound to a GNOME shortcut is the reliable path.

use std::{panic, thread, time::Duration};

use crossbeam_channel::Sender;
use device_query::{DeviceQuery, DeviceState, Keycode};

use crate::core::Command;

const POLL_INTERVAL_MS: u64 = 50;

pub fn spawn(command_tx: Sender<Command>) {
    thread::spawn(move || {
        // DeviceState::new panics when it cannot open the display; treat
        // that as "no hotkey support", not a daemon failure.
        let state = match panic::catch_unwind(DeviceState::new) {
            Ok(state) => state,
            Err(_) => {
                eprintln!("hotkey: no display connection, Meta+RAlt cycling disabled");
                return;
            }
        };

        let mut lock_switching = false;

        loop {
            let keys = state.get_keys();

            let combo_held = keys.contains(&Keycode::LMeta) && keys.contains(&Keycode::RAlt);
            if combo_held {
                if !lock_switching {
                    let send_result = command_tx.send(Command::CycleProfile);
                    if send_result.is_err() {
                        return; // Core is gone; daemon is shutting down.
                    }
                    lock_switching = true;
                }
            } else {
                lock_switching = false;
            }

            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
        }
    });
}
