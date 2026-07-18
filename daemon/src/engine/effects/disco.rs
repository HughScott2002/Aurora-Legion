use std::{sync::atomic::Ordering, thread, time::Duration};

use legion_kb_protocol::profile::Profile;
use rand::Rng;

use crate::engine::Inner;

pub fn play(manager: &mut Inner, p: &Profile, rng: &mut rand::rngs::ThreadRng) {
    while !manager.stop_signals.manager_stop_signal.load(Ordering::SeqCst) {
        let colors = [[255, 0, 0], [255, 255, 0], [0, 255, 0], [0, 255, 255], [0, 0, 255], [255, 0, 255]];
        let colors_index = rng.random_range(0..6);
        let new_values = colors[colors_index];

        let zone_index = rng.random_range(0..4);
        if !manager.write_zone(zone_index, new_values) {
            return;
        }
        thread::sleep(Duration::from_millis(2000 / (u64::from(p.speed) * 4)));
    }
}
