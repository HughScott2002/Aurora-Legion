use std::{
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};

use fast_image_resize as fr;

use fr::Resizer;
use scrap::{Capturer, Display, Frame, TraitCapturer, TraitPixelBuffer};

use crate::engine::Inner;

/// Wait this long before retrying when the screen capture setup fails.
/// Under Wayland the capture backend may be unavailable to a headless
/// daemon; the effect must degrade to "no output" instead of panicking.
const CAPTURE_RETRY_DELAY_MS: u64 = 2000;

#[derive(Clone, Copy)]
struct ScreenDimensions {
    src: (u32, u32),
    dest: (u32, u32),
}

pub fn play(manager: &mut Inner, fps: u8, saturation_boost: f32) {
    debug_assert!((1..=60).contains(&fps));
    debug_assert!((0.0..=1.0).contains(&saturation_boost));

    while !manager.stop_signals.manager_stop_signal.load(Ordering::SeqCst) {
        // Display setup. Failure here is expected on Wayland sessions where
        // the daemon has no capture access: log once per retry and wait.
        let mut displays = match Display::all() {
            Ok(displays) => displays,
            Err(error) => {
                eprintln!("ambient: could not enumerate displays: {error}");
                sleep_before_retry(manager);
                continue;
            }
        };

        if displays.is_empty() {
            eprintln!("ambient: no displays available for capture");
            sleep_before_retry(manager);
            continue;
        }

        let display = displays.remove(0);

        let mut capturer = match Capturer::new(display) {
            Ok(capturer) => capturer,
            Err(error) => {
                eprintln!("ambient: could not begin capture: {error}");
                sleep_before_retry(manager);
                continue;
            }
        };

        let dimensions = ScreenDimensions {
            src: (capturer.width() as u32, capturer.height() as u32),
            dest: (4, 1),
        };

        let seconds_per_frame = Duration::from_nanos(1_000_000_000 / u64::from(fps));
        let mut resizer = fr::Resizer::new();

        while !manager.stop_signals.keyboard_stop_signal.load(Ordering::SeqCst) {
            let now = Instant::now();

            match capturer.frame(seconds_per_frame) {
                Ok(frame) => {
                    let processed = process_frame(frame, dimensions, &mut resizer, saturation_boost);

                    match processed {
                        Some(rgb) => {
                            if !manager.write_colors(&rgb) {
                                return;
                            }
                        }
                        None => {
                            // Frame processing failed; skip this frame.
                        }
                    }
                }
                Err(error) => {
                    if error.kind() != std::io::ErrorKind::WouldBlock {
                        eprintln!("ambient: capture error: {error}");
                    }
                }
            }

            let elapsed_time = now.elapsed();
            if elapsed_time < seconds_per_frame {
                thread::sleep(seconds_per_frame - elapsed_time);
            }
        }
    }
}

fn sleep_before_retry(manager: &Inner) {
    let deadline = Instant::now() + Duration::from_millis(CAPTURE_RETRY_DELAY_MS);
    while Instant::now() < deadline {
        if manager.stop_signals.manager_stop_signal.load(Ordering::SeqCst) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn process_frame(frame: Frame, dimensions: ScreenDimensions, resizer: &mut Resizer, saturation_boost: f32) -> Option<[u8; 12]> {
    let Frame::PixelBuffer(buf) = frame else {
        eprintln!("ambient: got a texture frame, expected a pixel buffer");
        return None;
    };

    let frame_vec = buf.data().to_vec();

    let src_image = match fr::images::Image::from_vec_u8(dimensions.src.0, dimensions.src.1, frame_vec, fr::PixelType::U8x4) {
        Ok(image) => image,
        Err(error) => {
            eprintln!("ambient: could not wrap frame for resizing: {error}");
            return None;
        }
    };

    // Resize the whole screen down to one pixel per keyboard zone.
    let mut dst_image = fr::images::Image::new(dimensions.dest.0, dimensions.dest.1, fr::PixelType::U8x4);
    let resize_result = resizer.resize(&src_image, &mut dst_image, None);
    if let Err(error) = resize_result {
        eprintln!("ambient: resize failed: {error}");
        return None;
    }

    let bgra_arr = dst_image.buffer();

    // BGRA -> RGBA, one explicit copy per channel.
    let mut rgba: [u8; 16] = [0; 16];
    for (src, dst) in bgra_arr.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }

    let mut img = photon_rs::PhotonImage::new(rgba.to_vec(), 4, 1);
    photon_rs::colour_spaces::saturate_hsv(&mut img, saturation_boost);

    // RGBA -> RGB: drop the alpha channel.
    let raw = img.get_raw_pixels();
    let mut rgb: [u8; 12] = [0; 12];
    for (src, dst) in raw.chunks_exact(4).zip(rgb.chunks_exact_mut(3)) {
        dst[0] = src[0];
        dst[1] = src[1];
        dst[2] = src[2];
    }

    Some(rgb)
}
