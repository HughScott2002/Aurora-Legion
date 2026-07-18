use std::{sync::atomic::Ordering, thread, time::Duration};

use aurora_protocol::profile::Profile;
use rand::{rngs::ThreadRng, Rng};

use crate::engine::Inner;

pub fn play(manager: &mut Inner, p: &Profile, rng: &mut ThreadRng) {
    while !manager.stop_signals.manager_stop_signal.load(Ordering::SeqCst) {
        let profile_array = p.rgb_array();

        if manager.stop_signals.manager_stop_signal.load(Ordering::SeqCst) {
            break;
        }
        let zone_index = rng.random_range(0..4);
        let steps = rng.random_range(50..=200);

        let mut arr = [0; 12];
        let zone_start = zone_index * 3;

        arr[zone_start] = profile_array[zone_start];
        arr[zone_start + 1] = profile_array[zone_start + 1];
        arr[zone_start + 2] = profile_array[zone_start + 2];

        if !manager.write_colors(&arr) {
            return;
        }
        if !manager.write_transition(&[0; 12], steps / p.speed, 5) {
            return;
        }
        let sleep_time = rng.random_range(100..=2000);
        thread::sleep(Duration::from_millis(sleep_time));
    }
}
