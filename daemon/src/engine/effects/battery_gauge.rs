//! The Battery effect: the keyboard as a charge gauge.
//!
//! The effect picks no colors of its own. It takes the slot the user set up
//! and dims it zone by zone from the right, so a keyboard that was orange
//! stays orange and only loses ground as the battery drains. The maths for
//! that lives in [`Lighting::battery_gauge_array`], because the app draws
//! the same gauge in its preview and the two must agree.
//!
//! Reading the battery here rather than having the core push readings in
//! keeps the core out of the effect's business: a push would have to arrive
//! as a whole new lighting message, which restarts the effect. The path is
//! found once at startup and handed to the engine, so this loop opens a
//! known file and never searches.

use std::{
    path::Path,
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};

use aurora_protocol::profile::Lighting;

use crate::{battery, engine::Inner};

/// How often the charge is re-read. The gauge only moves when the reported
/// percent moves, which on a laptop is minutes apart, so a faster poll buys
/// nothing and a slower one lets the keyboard lag the battery.
const READ_INTERVAL: Duration = Duration::from_secs(5);

/// How long the loop sleeps between stop-signal checks. The engine's
/// contract is that a running effect unwinds within tens of milliseconds of
/// being replaced, so the charge deadline is checked across many short
/// sleeps rather than slept through in one long one.
const STOP_CHECK_INTERVAL: Duration = Duration::from_millis(100);

pub fn play(manager: &mut Inner, lighting: &Lighting) {
    let Some(battery_dir) = manager.battery_dir.clone() else {
        // Unreachable in normal use: the daemon refuses this effect without
        // a battery and clients do not offer it. Show the slot as the user
        // set it up rather than leaving the keyboard on whatever the last
        // effect left behind.
        eprintln!("engine: the Battery effect needs a battery, showing the slot unchanged");
        manager.write_colors(&lighting.rgb_array());
        return;
    };

    // None until the first successful read, so the first reading always
    // draws even if it happens to be a charge already on screen.
    let mut drawn_percent: Option<u8> = None;
    let mut read_at = Instant::now();

    while !manager
        .stop_signals
        .manager_stop_signal
        .load(Ordering::SeqCst)
    {
        if Instant::now() >= read_at {
            read_at = Instant::now() + READ_INTERVAL;
            if !draw_charge(manager, lighting, &battery_dir, &mut drawn_percent) {
                return;
            }
        }

        thread::sleep(STOP_CHECK_INTERVAL);
    }
}

/// Read the charge and redraw if it moved. Returns false when the keyboard
/// write failed and the effect must stop.
///
/// A failed read leaves the keyboard showing the last charge it knew. The
/// alternative is blanking the gauge on a transient sysfs error, which
/// reads as a dead battery.
fn draw_charge(
    manager: &mut Inner,
    lighting: &Lighting,
    battery_dir: &Path,
    drawn_percent: &mut Option<u8>,
) -> bool {
    let Some(reading) = battery::read(battery_dir) else {
        return true;
    };

    if *drawn_percent == Some(reading.percent) {
        return true;
    }

    let colors = lighting.battery_gauge_array(reading.percent);
    if !manager.write_colors(&colors) {
        return false;
    }

    *drawn_percent = Some(reading.percent);
    true
}
