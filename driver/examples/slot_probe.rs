//! Diagnostic for the EC slot counter. Two modes:
//!
//! - `cargo run -p legion-rgb-driver --example slot_probe poll`: read the
//!   counter every 50 ms for 8 seconds, printing every change. No writes.
//! - `... slot_probe write`: read the counter, send ONE static-color
//!   feature report (the same 0xCC 0x16 payload the daemon sends), then
//!   keep reading for 4 seconds to see whether the write moved the counter.
//!
//! Must run while the daemon is stopped (single libusb claim).

use hidapi::HidApi;
use std::time::{Duration, Instant};

const VENDOR_ID: u16 = 0x048d;
const PRODUCT_IDS: [u16; 11] = [
    0xc995, 0xc994, 0xc993, 0xc985, 0xc984, 0xc983, 0xc975, 0xc973, 0xc965, 0xc963, 0xc955,
];

const POLL_INTERVAL_MS: u64 = 50;

fn read_counter(device: &hidapi::HidDevice) -> Result<u8, String> {
    let mut report = [0u8; 33];
    report[0] = 0xcc;
    match device.get_feature_report(&mut report) {
        Ok(count) if count >= 2 => Ok(report[1]),
        Ok(count) => Err(format!("short report ({count} bytes)")),
        Err(error) => Err(error.to_string()),
    }
}

fn poll_for(device: &hidapi::HidDevice, seconds: u64, label: &str) {
    let start = Instant::now();
    let mut last: Option<Result<u8, String>> = None;
    while start.elapsed() < Duration::from_secs(seconds) {
        let reading = read_counter(device);
        if last.as_ref() != Some(&reading) {
            let elapsed = start.elapsed().as_secs_f64();
            match &reading {
                Ok(value) => println!("[{label} {elapsed:7.3}s] counter = {value}"),
                Err(message) => println!("[{label} {elapsed:7.3}s] read error: {message}"),
            }
            last = Some(reading);
        }
        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    }
}

fn main() {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "poll".to_string());

    let api = HidApi::new().expect("hidapi init failed");
    let info = api
        .device_list()
        .find(|device| {
            device.vendor_id() == VENDOR_ID && PRODUCT_IDS.contains(&device.product_id())
        })
        .expect("no supported keyboard found");
    let device = info
        .open_device(&api)
        .expect("could not open the keyboard (daemon still running?)");

    match mode.as_str() {
        "poll" => {
            println!("polling read-only for 8s, no writes");
            poll_for(&device, 8, "baseline");
        }
        "write" => {
            println!("counter before write:");
            poll_for(&device, 1, "before");

            // The exact payload shape the daemon sends: Static, speed 1,
            // low brightness, four orange zones.
            let mut payload = [0u8; 33];
            payload[0] = 0xcc;
            payload[1] = 0x16;
            payload[2] = 0x01;
            payload[3] = 1;
            payload[4] = 1;
            for zone_index in 0..4 {
                payload[5 + zone_index * 3] = 255;
                payload[6 + zone_index * 3] = 120;
                payload[7 + zone_index * 3] = 0;
            }
            device.send_feature_report(&payload).expect("write failed");
            println!("one static write sent (orange), watching counter for 4s:");
            poll_for(&device, 4, "after");
        }
        other => {
            eprintln!("unknown mode '{other}', use 'poll' or 'write'");
        }
    }
}
