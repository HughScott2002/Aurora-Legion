//! Reading the battery, for the low-battery alert.
//!
//! Two facts about this hardware shape the module:
//!
//! - A machine either has a battery or it does not, and that never changes
//!   while the daemon runs. So the search happens once at startup and the
//!   result is a path the core keeps; a desktop pays for one failed
//!   directory listing and nothing after it.
//! - Legion conservation mode parks the battery at a charge ceiling and
//!   reports `Not charging` while the laptop is on AC. Only `Discharging`
//!   means the battery is actually draining, which is why
//!   [`Reading::discharging`] tests for that word rather than for the
//!   absence of `Charging`.
//!
//! Both functions take the directory to read, so the tests point them at
//! fixtures instead of at the running machine.

use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

/// Where the kernel exposes power supplies. The batteries among them are
/// the entries named `BAT*`; the rest are AC adapters and USB-C sources.
const POWER_SUPPLY_DIR: &str = "/sys/class/power_supply";

/// Marks a power supply directory as a battery rather than an adapter.
const BATTERY_DIR_PREFIX: &str = "BAT";

/// Longest sysfs value this module accepts. These files hold a small
/// integer or a single word, so anything longer is not something it
/// understands and is rejected rather than parsed.
const MAX_VALUE_BYTES: u64 = 64;

/// The highest charge percentage the kernel can legitimately report.
const MAX_PERCENT: u8 = 100;

/// One sample of the battery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// Charge percentage, 0 to 100.
    pub percent: u8,
    /// True only while the battery is actually draining. Being off AC is
    /// the thing that matters here, not the absence of charging: see the
    /// module docs on conservation mode.
    pub discharging: bool,
}

/// Find the battery to watch, or `None` on a machine without one. Called
/// once at startup; the core holds the result for the life of the process.
pub fn probe() -> Option<PathBuf> {
    probe_in(Path::new(POWER_SUPPLY_DIR))
}

/// Read one sample, or `None` when the files are missing or hold something
/// unparseable. A failed read is not fatal: the caller keeps its previous
/// answer and tries again on the next poll.
pub fn read(battery_dir: &Path) -> Option<Reading> {
    let capacity_text = read_value(&battery_dir.join("capacity"))?;
    let percent: u8 = capacity_text.parse().ok()?;
    // Kernel input, so reject it rather than assert on it.
    if percent > MAX_PERCENT {
        return None;
    }

    let status_text = read_value(&battery_dir.join("status"))?;
    let discharging = status_text == "Discharging";

    Some(Reading {
        percent,
        discharging,
    })
}

fn probe_in(power_supply_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(power_supply_dir).ok()?;

    let mut battery_dirs: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !name.starts_with(BATTERY_DIR_PREFIX) {
            continue;
        }
        battery_dirs.push(entry.path());
    }

    // Directory order is not defined, so sort: a machine with two
    // batteries must watch the same one on every start.
    battery_dirs.sort();

    // Prove the choice by reading it. A BAT entry whose files are missing
    // or unreadable is no use, and the next one might work.
    battery_dirs
        .into_iter()
        .find(|battery_dir| read(battery_dir).is_some())
}

fn read_value(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;

    let mut text = String::new();
    let mut bounded = file.take(MAX_VALUE_BYTES);
    bounded.read_to_string(&mut text).ok()?;

    Some(text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    /// Build a power supply tree under a fresh temporary directory. Each
    /// entry is a directory name with the `capacity` and `status` contents
    /// to write into it; `None` leaves the file out entirely.
    fn power_supply_tree(name: &str, entries: &[(&str, Option<&str>, Option<&str>)]) -> PathBuf {
        let mut root = std::env::temp_dir();
        root.push(format!("aurora-battery-test-{name}"));

        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test fixture root");

        for (dir_name, capacity, status) in entries {
            let entry_dir = root.join(dir_name);
            fs::create_dir_all(&entry_dir).expect("test fixture entry");

            if let Some(capacity) = capacity {
                fs::write(entry_dir.join("capacity"), capacity).expect("test fixture capacity");
            }
            if let Some(status) = status {
                fs::write(entry_dir.join("status"), status).expect("test fixture status");
            }
        }

        root
    }

    #[test]
    fn a_discharging_battery_reads_its_charge() {
        let root = power_supply_tree(
            "discharging",
            &[("BAT0", Some("12\n"), Some("Discharging\n"))],
        );

        let battery_dir = probe_in(&root).expect("BAT0 is a battery");
        let reading = read(&battery_dir).expect("readable");

        assert_eq!(reading.percent, 12);
        assert!(reading.discharging);
    }

    /// Conservation mode reports `Not charging` on AC. Treating that as
    /// discharging would light the alert while the laptop is plugged in.
    #[test]
    fn not_charging_is_not_discharging() {
        let root = power_supply_tree(
            "conservation",
            &[("BAT0", Some("76\n"), Some("Not charging\n"))],
        );

        let battery_dir = probe_in(&root).expect("BAT0 is a battery");
        let reading = read(&battery_dir).expect("readable");

        assert_eq!(reading.percent, 76);
        assert!(!reading.discharging);
    }

    #[test]
    fn charging_is_not_discharging() {
        let root = power_supply_tree("charging", &[("BAT0", Some("40\n"), Some("Charging\n"))]);

        let battery_dir = probe_in(&root).expect("BAT0 is a battery");
        let reading = read(&battery_dir).expect("readable");

        assert!(!reading.discharging);
    }

    #[test]
    fn a_machine_without_a_battery_finds_none() {
        let root = power_supply_tree(
            "desktop",
            &[
                ("ADP0", None, None),
                ("ucsi-source-psy-USBC000:001", None, None),
            ],
        );

        assert!(probe_in(&root).is_none());
    }

    #[test]
    fn a_missing_power_supply_directory_finds_none() {
        let mut root = std::env::temp_dir();
        root.push("aurora-battery-test-absent");
        let _ = fs::remove_dir_all(&root);

        assert!(probe_in(&root).is_none());
    }

    #[test]
    fn a_battery_missing_its_capacity_is_skipped_for_one_that_reads() {
        let root = power_supply_tree(
            "two-batteries",
            &[
                ("BAT0", None, Some("Discharging\n")),
                ("BAT1", Some("55\n"), Some("Discharging\n")),
            ],
        );

        let battery_dir = probe_in(&root).expect("BAT1 reads");
        assert!(battery_dir.ends_with("BAT1"));
    }

    #[test]
    fn a_charge_above_one_hundred_is_rejected() {
        let root = power_supply_tree(
            "impossible",
            &[("BAT0", Some("140\n"), Some("Discharging\n"))],
        );

        assert!(read(&root.join("BAT0")).is_none());
    }

    #[test]
    fn an_unparseable_charge_is_rejected() {
        let root = power_supply_tree(
            "garbage",
            &[("BAT0", Some("full\n"), Some("Discharging\n"))],
        );

        assert!(read(&root.join("BAT0")).is_none());
    }
}
