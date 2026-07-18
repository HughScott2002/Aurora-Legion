use std::{sync::atomic::Ordering, thread, time::Duration};

use rand::Rng;

use crate::engine::Inner;

pub fn play(manager: &mut Inner, rng: &mut rand::rngs::ThreadRng) {
    let xmas_color_array = [[255, 10, 10], [255, 255, 20], [30, 255, 30], [70, 70, 255]];
    let subeffect_count = 4;
    let mut last_subeffect = -1;
    while !manager.stop_signals.manager_stop_signal.load(Ordering::SeqCst) {
        let mut subeffect = rng.random_range(0..subeffect_count);
        while last_subeffect == subeffect {
            subeffect = rng.random_range(0..subeffect_count);
        }
        last_subeffect = subeffect;

        match subeffect {
            0 => {
                for _i in 0..3 {
                    for colors in xmas_color_array {
                        if !manager.write_solid_colors(colors) {
                            return;
                        }
                        thread::sleep(Duration::from_millis(500));
                    }
                }
            }
            1 => {
                let color_1_index = rng.random_range(0..4);
                let used_colors_1: [u8; 3] = xmas_color_array[color_1_index];

                let mut color_2_index = rng.random_range(0..4);
                while color_1_index == color_2_index {
                    color_2_index = rng.random_range(0..4);
                }
                let used_colors_2: [u8; 3] = xmas_color_array[color_2_index];

                for _i in 0..4 {
                    if !manager.write_solid_colors(used_colors_1) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(400));
                    if !manager.write_solid_colors(used_colors_2) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(400));
                }
            }
            2 => {
                let steps = 100;
                if !manager.write_transition(&[0; 12], steps, 1) {
                    return;
                }
                let mut used_colors_array: [u8; 12] = [0; 12];
                let left_or_right = rng.random_range(0..2);

                let zone_order: Vec<usize> = if left_or_right == 0 { (0..4).collect() } else { (0..4).rev().collect() };

                for color in xmas_color_array {
                    for target_zone in zone_order.clone() {
                        used_colors_array[target_zone * 3] = color[0];
                        used_colors_array[target_zone * 3 + 1] = color[1];
                        used_colors_array[target_zone * 3 + 2] = color[2];
                        if !manager.write_transition(&used_colors_array, steps, 1) {
                            return;
                        }
                    }
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
            3 => {
                let state1 = [255, 255, 255, 0, 0, 0, 255, 255, 255, 0, 0, 0];
                let state2 = [0, 0, 0, 255, 255, 255, 0, 0, 0, 255, 255, 255];
                let steps = 30;
                for _i in 0..4 {
                    if !manager.write_transition(&state1, steps, 1) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(400));
                    if !manager.write_transition(&state2, steps, 1) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(400));
                }
            }
            _ => unreachable!("Subeffect index for Christmas effect is out of range."),
        }
    }
}
