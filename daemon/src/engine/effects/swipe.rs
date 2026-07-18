use std::{sync::atomic::Ordering, thread, time::Duration};

use legion_kb_protocol::{
    effects::{Direction, SwipeMode},
    profile::Profile,
};

use crate::engine::Inner;

const STEPS: u8 = 150;

pub fn play(manager: &mut Inner, profile: &Profile, mode: SwipeMode, clean_with_black: bool) {
    let mut change_rgb_array = profile.rgb_array();
    let fill_rgb_array = profile.rgb_array();
    // Placed here so we don't reset it every loop
    let mut used_colors_array: [u8; 12] = [0; 12];

    let steps = STEPS / profile.speed;

    while !manager.stop_signals.manager_stop_signal.load(Ordering::SeqCst) {
        match mode {
            SwipeMode::Change => {
                match profile.direction {
                    Direction::Left => change_rgb_array.rotate_right(3),
                    Direction::Right => change_rgb_array.rotate_left(3),
                }
                if !manager.write_transition(&change_rgb_array, steps, 10) {
                    return;
                }
            }
            SwipeMode::Fill => {
                let zone_order: Vec<usize> = match profile.direction {
                    Direction::Left => (0..4).collect(),
                    Direction::Right => (0..4).rev().collect(),
                };

                for source_zone in zone_order.clone() {
                    for target_zone in zone_order.clone() {
                        used_colors_array[target_zone * 3] = fill_rgb_array[source_zone * 3];
                        used_colors_array[target_zone * 3 + 1] = fill_rgb_array[source_zone * 3 + 1];
                        used_colors_array[target_zone * 3 + 2] = fill_rgb_array[source_zone * 3 + 2];
                        if !manager.write_transition(&used_colors_array, steps, 1) {
                            return;
                        }
                    }
                    if clean_with_black {
                        for target_zone in zone_order.clone() {
                            used_colors_array[target_zone * 3] = 0;
                            used_colors_array[target_zone * 3 + 1] = 0;
                            used_colors_array[target_zone * 3 + 2] = 0;
                            if !manager.write_transition(&used_colors_array, steps, 1) {
                                return;
                            }
                        }
                    }
                }
            }
        }

        thread::sleep(Duration::from_millis(20));
    }
}
