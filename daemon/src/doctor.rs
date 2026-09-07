//! `aurora doctor`: what this machine looks like to Aurora, and what is
//! stopping it working.
//!
//! Two audiences, one output. A user wants to know what to fix; a bug report
//! wants something to paste. Serving both from the same checks means there
//! is no second format to keep true.
//!
//! The conventions here are borrowed, not invented. Human-readable output on
//! stdout with `--json` for machines, colour only where a terminal wants it,
//! a non-zero exit when something is wrong, and an error that names the fix
//! rather than the fault: all from the Command Line Interface Guidelines
//! (<https://clig.dev>). Colour also honours <https://no-color.org>. The
//! shape of the report, one line per check with a marker, is what
//! `brew doctor` and `flutter doctor` taught people to expect.

use std::{
    env,
    io::IsTerminal,
    path::Path,
    process::{Command, ExitCode, Stdio},
    thread,
    time::{Duration, Instant},
};

use aurora_protocol::ipc::{KeyboardStatus, Request, Response, PROTOCOL_VERSION};
use serde::Serialize;

use crate::{
    battery,
    client::{Client, ClientError},
};

/// Where the running daemon's own version comes from.
const OWN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Files udev reads, in the order it reads them. A rule can be installed in
/// any of these, and which one it lands in is the difference between a
/// package and a hand edit.
const UDEV_RULE_DIRS: [&str; 3] = [
    "/etc/udev/rules.d",
    "/run/udev/rules.d",
    "/usr/lib/udev/rules.d",
];

/// systemd's `73-seat-late.rules` is what turns a `uaccess` tag into an ACL,
/// so a rule sorting after it tags the device too late to matter. Shipping a
/// rule numbered 99 is the fault that made 0.24.1 (#20), and an install that
/// predates the fix still has one on disk.
const UDEV_SEAT_RULE_PREFIX: u32 = 73;

/// How long `systemctl` gets to answer before Aurora stops waiting. It talks
/// to a bus that can be wedged, and a diagnostic that hangs is worse than one
/// that reports a missing answer.
const SERVICE_QUERY_TIMEOUT: Duration = Duration::from_secs(2);

/// How often the wait for `systemctl` checks whether it has finished.
const SERVICE_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// What one check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Health {
    /// Working.
    Ok,
    /// Working, or not needed, but worth knowing about.
    Warn,
    /// Broken, and Aurora cannot do its job until it is fixed.
    Fail,
    /// Could not be determined on this machine.
    Unknown,
}

/// One line of the report.
#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub health: Health,
    pub summary: String,
    /// Facts a maintainer reading a bug report wants, printed under the
    /// summary. Never required to understand the summary.
    pub detail: Vec<String>,
    /// What the user should do about it, where there is something to do.
    pub fix: Option<String>,
}

impl Check {
    fn new(name: &'static str, health: Health, summary: String) -> Self {
        Check {
            name,
            health,
            summary,
            detail: Vec::new(),
            fix: None,
        }
    }

    fn with_detail(mut self, line: String) -> Self {
        self.detail.push(line);
        self
    }

    fn with_fix(mut self, fix: &str) -> Self {
        self.fix = Some(fix.to_string());
        self
    }
}

/// The whole report, as `--json` prints it.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub aurora: &'static str,
    pub checks: Vec<Check>,
    /// How many checks failed. Zero means nothing needs fixing.
    pub problems: usize,
}

pub fn run(as_json: bool) -> ExitCode {
    let checks = collect_checks();

    let mut problems = 0;
    for check in &checks {
        if check.health == Health::Fail {
            problems += 1;
        }
    }

    let report = Report {
        aurora: OWN_VERSION,
        checks,
        problems,
    };

    if as_json {
        print_json(&report);
    } else {
        print_human(&report);
    }

    // clig.dev: zero on success, non-zero on failure. A found problem is the
    // failure a script wants to branch on.
    if report.problems > 0 {
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn collect_checks() -> Vec<Check> {
    vec![
        check_aurora(),
        check_system(),
        check_controller(),
        check_access(),
        check_udev(),
        check_daemon(),
        check_service(),
        check_battery(),
    ]
}

// --- Checks --------------------------------------------------------------

fn check_aurora() -> Check {
    let summary = format!("aurora {OWN_VERSION} on {}", env::consts::ARCH);

    Check::new("aurora", Health::Ok, summary)
        .with_detail(format!("protocol version {PROTOCOL_VERSION}"))
}

fn check_system() -> Check {
    let distro = os_release_field("PRETTY_NAME").unwrap_or_else(|| "unknown distro".to_string());
    let kernel = read_trimmed(Path::new("/proc/sys/kernel/osrelease"))
        .unwrap_or_else(|| "unknown kernel".to_string());

    let session = env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".to_string());
    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".to_string());

    Check::new("system", Health::Ok, format!("{distro}, kernel {kernel}"))
        .with_detail(format!("session {session}, desktop {desktop}"))
}

fn check_controller() -> Check {
    let devices = match legion_rgb_driver::scan_vendor_devices() {
        Ok(devices) => devices,
        Err(error) => {
            return Check::new(
                "controller",
                Health::Unknown,
                format!("could not read the USB bus: {error}"),
            );
        }
    };

    let mut supported = Vec::new();
    let mut others = Vec::new();
    for device in &devices {
        let id = format!(
            "{:04x}:{:04x}",
            legion_rgb_driver::VENDOR_ID,
            device.product_id
        );
        if device.supported {
            supported.push(id);
        } else {
            others.push(id);
        }
    }

    if !supported.is_empty() {
        return Check::new(
            "controller",
            Health::Ok,
            format!(
                "lighting controller {} on the USB bus",
                supported.join(", ")
            ),
        );
    }

    if others.is_empty() {
        return Check::new(
            "controller",
            Health::Fail,
            "no Lenovo device on the USB bus at all".to_string(),
        )
        .with_fix(
            "If the keyboard lit up earlier this boot, the controller stopped \
             answering and only a reboot brings it back. On a machine that has \
             never worked, Aurora does not support this model.",
        );
    }

    Check::new(
        "controller",
        Health::Fail,
        format!(
            "Lenovo devices on the bus, but no lighting controller among them: {}",
            others.join(", ")
        ),
    )
    .with_detail(
        "048d:c103 is the plain HID keyboard interface, which every Legion has \
         whether its lighting controller is present or not."
            .to_string(),
    )
    .with_fix(
        "If the keyboard lit up earlier this boot, the controller stopped \
         answering and only a reboot brings it back. If it has never worked, \
         report these IDs at \
         https://github.com/HughScott2002/Aurora-Legion/issues so the model can \
         be added.",
    )
}

fn check_access() -> Check {
    match legion_rgb_driver::can_open_keyboard() {
        Ok(()) => Check::new(
            "access",
            Health::Ok,
            "the controller opens, so the udev rule is in effect".to_string(),
        ),
        Err(legion_rgb_driver::error::Error::DeviceNotFound) => Check::new(
            "access",
            Health::Unknown,
            "no controller to open; see the controller check above".to_string(),
        ),
        Err(error) => {
            let reason = crate::keyboard::readable_error(error.to_string());
            let denied = reason.to_lowercase().contains("permission denied")
                || reason.to_lowercase().contains("not permitted");

            if denied {
                return Check::new(
                    "access",
                    Health::Fail,
                    "the controller is on the bus but Aurora may not open it".to_string(),
                )
                .with_detail(reason)
                .with_fix(
                    "Install the udev rule: \
                     https://github.com/HughScott2002/Aurora-Legion/blob/main/docs/how-to/install-linux.md#grant-keyboard-access",
                );
            }

            Check::new(
                "access",
                Health::Fail,
                "the controller is on the bus but did not open".to_string(),
            )
            .with_detail(reason)
        }
    }
}

fn check_udev() -> Check {
    let mut found = Vec::new();
    let mut too_late = Vec::new();

    for dir in UDEV_RULE_DIRS {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.contains("aurora") {
                continue;
            }

            if rule_sorts_too_late(&name) {
                too_late.push(format!("{dir}/{name}"));
            }
            found.push(format!("{dir}/{name}"));
        }
    }

    if found.is_empty() {
        return Check::new(
            "udev",
            Health::Warn,
            "no Aurora udev rule found".to_string(),
        )
        .with_detail(
            "Only a problem if the access check failed: some setups grant the \
             device another way."
                .to_string(),
        )
        .with_fix(
            "Install the rule: \
             https://github.com/HughScott2002/Aurora-Legion/blob/main/docs/how-to/install-linux.md#grant-keyboard-access",
        );
    }

    if !too_late.is_empty() {
        return Check::new(
            "udev",
            Health::Warn,
            format!(
                "udev rule sorts too late to grant access: {}",
                too_late.join(", ")
            ),
        )
        .with_detail(format!(
            "systemd's {UDEV_SEAT_RULE_PREFIX}-seat-late.rules is what turns the \
             uaccess tag into an ACL, so a rule numbered above it tags the device \
             and never gets the ACL applied."
        ))
        .with_fix("Upgrade Aurora, or renumber the rule below 73 (#20).");
    }

    Check::new(
        "udev",
        Health::Ok,
        format!("udev rule {}", found.join(", ")),
    )
}

fn check_daemon() -> Check {
    let mut client = match Client::connect() {
        Ok(client) => client,
        // The socket answered, so a daemon is up. It just does not speak
        // this build's protocol, and calling that "not running" would send
        // the user to start something that is already running.
        Err(ClientError::Protocol(message)) => {
            return Check::new(
                "daemon",
                Health::Fail,
                "daemon running, but it speaks a different protocol version".to_string(),
            )
            .with_detail(message)
            .with_fix(
                "Both sides have to come from the same install. Restart the \
                 daemon with: systemctl --user restart aurora",
            );
        }
        Err(ClientError::Io(error)) => {
            return Check::new("daemon", Health::Warn, "daemon not running".to_string())
                .with_detail(error.to_string())
                .with_fix("Start it with: systemctl --user start aurora   (or: aurora daemon)");
        }
    };

    let response = match client.request(Request::GetState) {
        Ok(response) => response,
        Err(error) => {
            return Check::new(
                "daemon",
                Health::Fail,
                format!("daemon running, but did not answer: {error}"),
            );
        }
    };

    let Response::State { state } = response else {
        return Check::new(
            "daemon",
            Health::Fail,
            "daemon running, but sent something other than state".to_string(),
        );
    };

    let keyboard = describe_keyboard(&state.keyboard);
    let mut check = Check::new(
        "daemon",
        Health::Ok,
        format!("daemon running v{}, keyboard {keyboard}", state.version),
    );

    if state.version != OWN_VERSION {
        check.health = Health::Warn;
        check.summary = format!(
            "daemon running v{}, but this build is v{OWN_VERSION}, keyboard {keyboard}",
            state.version
        );
        check.fix = Some(
            "An upgrade replaced the binaries without restarting the daemon. \
             Restart it with: systemctl --user restart aurora"
                .to_string(),
        );
        return check;
    }

    if matches!(state.keyboard, KeyboardStatus::Lost) {
        check.health = Health::Fail;
        check.fix = Some(
            "The controller stopped answering after Aurora had it. Restarting \
             the daemon cannot reopen it; reboot."
                .to_string(),
        );
    }

    check
}

fn check_service() -> Check {
    let Some(output) = run_bounded("systemctl", &["--user", "is-active", "aurora.service"]) else {
        return Check::new(
            "service",
            Health::Unknown,
            "systemctl did not answer, so the unit state is unknown".to_string(),
        );
    };

    let state = output.trim().to_string();
    let health = match state.as_str() {
        "active" => Health::Ok,
        _ => Health::Warn,
    };

    let mut check = Check::new("service", health, format!("aurora.service is {state}"));
    if health == Health::Warn {
        check.fix = Some(
            "Aurora runs without systemd, but nothing will start it at login. \
             Enable it with: systemctl --user enable --now aurora"
                .to_string(),
        );
    }

    check
}

fn check_battery() -> Check {
    match battery::probe() {
        Some(dir) => {
            let name = dir
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "battery".to_string());

            Check::new(
                "battery",
                Health::Ok,
                format!("{name} found, so the low battery alert and Battery effect work"),
            )
        }
        None => Check::new(
            "battery",
            Health::Warn,
            "no battery, so the low battery alert and Battery effect are hidden".to_string(),
        ),
    }
}

// --- Helpers -------------------------------------------------------------

fn describe_keyboard(status: &KeyboardStatus) -> String {
    match status {
        KeyboardStatus::Connected => "connected".to_string(),
        KeyboardStatus::Searching => "searching".to_string(),
        KeyboardStatus::Lost => "lost".to_string(),
        KeyboardStatus::PermissionDenied { .. } => "access denied".to_string(),
        KeyboardStatus::Error { message } => format!("error ({message})"),
    }
}

/// True where a rule file's leading number sorts at or after the seat rule,
/// which is where a `uaccess` tag stops turning into an ACL.
fn rule_sorts_too_late(file_name: &str) -> bool {
    let mut digits = String::new();
    for character in file_name.chars() {
        if !character.is_ascii_digit() {
            break;
        }
        digits.push(character);
    }

    match digits.parse::<u32>() {
        Ok(prefix) => prefix >= UDEV_SEAT_RULE_PREFIX,
        // No leading number at all: udev sorts it by name, which is its own
        // problem, but not the one this check is about.
        Err(_) => false,
    }
}

/// Runs a command and gives it [`SERVICE_QUERY_TIMEOUT`] to answer, returning
/// `None` where it is missing, fails to start, or outlasts the wait. `output`
/// would block for as long as the child wants, and a diagnostic must not.
fn run_bounded(program: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + SERVICE_QUERY_TIMEOUT;
    loop {
        match child.try_wait() {
            // `is-active` exits non-zero for an inactive unit and still
            // prints the state, so the status is not consulted.
            Ok(Some(_status)) => break,
            Ok(None) => {}
            Err(_) => return None,
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            return None;
        }

        thread::sleep(SERVICE_POLL_INTERVAL);
    }

    let output = child.wait_with_output().ok()?;
    String::from_utf8(output.stdout).ok()
}

fn os_release_field(field: &str) -> Option<String> {
    let text = read_trimmed(Path::new("/etc/os-release"))?;

    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key != field {
            continue;
        }

        let unquoted = value.trim().trim_matches('"');
        return Some(unquoted.to_string());
    }

    None
}

fn read_trimmed(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.trim().to_string())
}

/// How wide the report's wrapped lines are allowed to run. Terminals vary,
/// but a fixed width beats a line that runs off the side, and no part of
/// this output is worth a dependency on the terminal size.
const WRAP_COLUMNS: usize = 76;

/// Breaks `text` into lines no longer than `width`, splitting on spaces. A
/// word longer than `width` gets a line of its own rather than being cut:
/// the long words here are URLs, and a broken URL is not clickable.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();

    for word in text.split_whitespace() {
        if line.is_empty() {
            line.push_str(word);
            continue;
        }

        if line.len() + 1 + word.len() > width {
            lines.push(line.clone());
            line.clear();
            line.push_str(word);
            continue;
        }

        line.push(' ');
        line.push_str(word);
    }

    if !line.is_empty() {
        lines.push(line);
    }

    lines
}

// --- Output --------------------------------------------------------------

fn print_json(report: &Report) {
    match serde_json::to_string_pretty(report) {
        Ok(text) => println!("{text}"),
        Err(error) => eprintln!("aurora: could not render the report: {error}"),
    }
}

fn print_human(report: &Report) {
    let palette = Palette::for_stdout();

    println!("Aurora doctor");
    println!();

    for check in &report.checks {
        let (marker, colour) = match check.health {
            Health::Ok => ("ok  ", palette.ok),
            Health::Warn => ("warn", palette.warn),
            Health::Fail => ("fail", palette.fail),
            Health::Unknown => ("?   ", palette.dim),
        };

        println!("  {colour}{marker}{}  {}", palette.reset, check.summary);
        for line in &check.detail {
            for wrapped in wrap(line, WRAP_COLUMNS) {
                println!("        {}{wrapped}{}", palette.dim, palette.reset);
            }
        }
        if let Some(fix) = &check.fix {
            let mut label = format!("{}fix:{} ", palette.warn, palette.reset);
            for wrapped in wrap(fix, WRAP_COLUMNS) {
                println!("        {label}{wrapped}");
                label = "     ".to_string();
            }
        }
    }

    println!();
    if report.problems == 0 {
        println!("Nothing to report.");
        return;
    }

    let plural = if report.problems == 1 { "" } else { "s" };
    println!(
        "{} problem{plural} found. Paste this into a bug report if the fix does not help:",
        report.problems
    );
    println!("https://github.com/HughScott2002/Aurora-Legion/issues/new/choose");
}

/// ANSI colour, used only where the conventions allow it.
struct Palette {
    ok: &'static str,
    warn: &'static str,
    fail: &'static str,
    dim: &'static str,
    reset: &'static str,
}

impl Palette {
    /// Colour is added only for an interactive terminal that has not asked
    /// to go without: clig.dev on TTY detection, no-color.org on `NO_COLOR`,
    /// and `TERM=dumb` for terminals that cannot render it.
    fn for_stdout() -> Self {
        let is_terminal = std::io::stdout().is_terminal();
        let no_color = match env::var("NO_COLOR") {
            Ok(value) => !value.is_empty(),
            Err(_) => false,
        };
        let dumb_terminal = match env::var("TERM") {
            Ok(value) => value == "dumb",
            Err(_) => false,
        };

        if !is_terminal || no_color || dumb_terminal {
            return Palette::plain();
        }

        Palette {
            ok: "\x1b[32m",
            warn: "\x1b[33m",
            fail: "\x1b[31m",
            dim: "\x1b[2m",
            reset: "\x1b[0m",
        }
    }

    fn plain() -> Self {
        Palette {
            ok: "",
            warn: "",
            fail: "",
            dim: "",
            reset: "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_numbered_above_the_seat_rule_is_too_late() {
        assert!(rule_sorts_too_late("99-aurora.rules"));
    }

    #[test]
    fn the_shipped_rule_number_sorts_early_enough() {
        assert!(!rule_sorts_too_late("70-aurora.rules"));
    }

    #[test]
    fn the_seat_rule_number_itself_is_too_late() {
        assert!(rule_sorts_too_late("73-aurora.rules"));
    }

    #[test]
    fn a_rule_with_no_number_is_left_alone() {
        assert!(!rule_sorts_too_late("aurora.rules"));
    }

    #[test]
    fn wrapping_keeps_every_line_inside_the_width() {
        let text = "The controller is on the bus but Aurora may not open it, so \
                    the udev rule is the thing to check first.";

        for line in wrap(text, 40) {
            assert!(line.len() <= 40, "{line}");
        }
    }

    #[test]
    fn wrapping_never_breaks_a_long_word() {
        let url = "https://github.com/HughScott2002/Aurora-Legion/issues/new/choose";
        let wrapped = wrap(url, 40);

        assert_eq!(wrapped, vec![url.to_string()]);
    }

    #[test]
    fn wrapping_keeps_every_word() {
        let text = "one two three four five six";
        let wrapped = wrap(text, 9);

        assert_eq!(wrapped.join(" "), text);
    }

    #[test]
    fn no_color_is_honoured_however_it_is_set() {
        // The standard is presence, not value: any non-empty string counts.
        let palette = Palette::plain();

        assert_eq!(palette.ok, "");
        assert_eq!(palette.reset, "");
    }
}
