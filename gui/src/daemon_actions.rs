//! Daemon lifecycle actions: systemctl queries and calls, plus the
//! no-systemd fallback (ask the daemon to exit over the socket, wait, spawn
//! a new one). All of it runs on short-lived threads so the GTK main loop
//! never blocks; results come back as [`AppMsg`] values.

use std::{
    io::Write,
    os::unix::net::UnixStream,
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use legion_kb_protocol::ipc::socket_path;

use crate::app::AppMsg;

pub const UNIT_NAME: &str = "legion-kb.service";

/// How long the fallback restart waits for the old daemon's socket to
/// disappear before spawning a new one.
const SHUTDOWN_WAIT_MAX: Duration = Duration::from_secs(5);
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(100);

pub fn query_autostart<F>(deliver: F)
where
    F: Fn(AppMsg) + Send + 'static,
{
    thread::spawn(move || {
        let fragment_path = read_unit_fragment_path();

        let Some(fragment_path) = fragment_path else {
            deliver(AppMsg::AutostartQueried {
                available: false,
                enabled: false,
                managed: false,
            });
            return;
        };

        let managed = fragment_path.starts_with("/nix/store");
        let enabled = unit_is_enabled();

        deliver(AppMsg::AutostartQueried {
            available: true,
            enabled,
            managed,
        });
    });
}

pub fn set_autostart<F>(enable: bool, deliver: F)
where
    F: Fn(AppMsg) + Send + 'static,
{
    thread::spawn(move || {
        let verb = if enable { "enable" } else { "disable" };
        let error = run_systemctl(&[verb, UNIT_NAME]);
        deliver(AppMsg::ServiceActionFinished {
            description: format!("{verb} {UNIT_NAME}"),
            error,
        });
    });
}

/// Restart via systemd when the unit exists; otherwise ask the daemon to
/// exit over the socket, wait until its socket is gone, and spawn a fresh
/// `legion-kb daemon`.
pub fn restart_daemon<F>(unit_available: bool, deliver: F)
where
    F: Fn(AppMsg) + Send + 'static,
{
    thread::spawn(move || {
        if unit_available {
            let error = run_systemctl(&["restart", UNIT_NAME]);
            deliver(AppMsg::ServiceActionFinished {
                description: format!("restart {UNIT_NAME}"),
                error,
            });
            return;
        }

        let error = restart_without_systemd();
        deliver(AppMsg::ServiceActionFinished {
            description: "restart daemon".to_string(),
            error,
        });
    });
}

/// Start the daemon (status page button): systemd first, spawn fallback.
pub fn start_daemon<F>(unit_available: bool, deliver: F)
where
    F: Fn(AppMsg) + Send + 'static,
{
    thread::spawn(move || {
        if unit_available {
            let error = run_systemctl(&["start", UNIT_NAME]);
            deliver(AppMsg::ServiceActionFinished {
                description: format!("start {UNIT_NAME}"),
                error,
            });
            return;
        }

        let error = spawn_daemon_process();
        deliver(AppMsg::ServiceActionFinished {
            description: "start daemon".to_string(),
            error,
        });
    });
}

// --- systemctl helpers ---------------------------------------------------

fn run_systemctl(args: &[&str]) -> Option<String> {
    let mut command = Command::new("systemctl");
    command.arg("--user");
    for arg in args {
        command.arg(arg);
    }

    let output = match command.output() {
        Ok(output) => output,
        Err(error) => return Some(format!("could not run systemctl: {error}")),
    };

    if output.status.success() {
        return None;
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Some(format!("systemctl --user {} failed: {stderr}", args.join(" ")))
}

fn read_unit_fragment_path() -> Option<String> {
    let output = Command::new("systemctl")
        .args(["--user", "show", "-p", "FragmentPath", "--value", UNIT_NAME])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }
    Some(path)
}

fn unit_is_enabled() -> bool {
    let output = Command::new("systemctl").args(["--user", "is-enabled", UNIT_NAME]).output();

    match output {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

// --- No-systemd fallback -------------------------------------------------

fn restart_without_systemd() -> Option<String> {
    let shutdown_error = send_shutdown_request();
    if let Some(error) = shutdown_error {
        return Some(error);
    }

    let socket = socket_path();
    let deadline = Instant::now() + SHUTDOWN_WAIT_MAX;
    while socket.exists() {
        if Instant::now() > deadline {
            return Some("old daemon did not exit in time".to_string());
        }
        thread::sleep(SHUTDOWN_POLL_INTERVAL);
    }

    spawn_daemon_process()
}

fn send_shutdown_request() -> Option<String> {
    let stream = match UnixStream::connect(socket_path()) {
        Ok(stream) => stream,
        // No daemon to shut down; skip straight to spawning.
        Err(_) => return None,
    };

    let mut writer = stream;
    let request = r#"{"id":1,"req":{"type":"Shutdown"}}"#;
    let write_result = writeln!(writer, "{request}");
    match write_result {
        Ok(()) => None,
        Err(error) => Some(format!("could not send shutdown request: {error}")),
    }
}

fn spawn_daemon_process() -> Option<String> {
    let binary = find_daemon_binary();
    let spawn_result = Command::new(&binary).arg("daemon").spawn();
    match spawn_result {
        Ok(_) => None,
        Err(error) => Some(format!("could not start {}: {error}", binary.display())),
    }
}

/// Prefer the `legion-kb` binary that ships next to this GUI binary (nix
/// store or target dir); fall back to $PATH.
fn find_daemon_binary() -> PathBuf {
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            let sibling = dir.join("legion-kb");
            if sibling.is_file() {
                return sibling;
            }
        }
    }

    PathBuf::from("legion-kb")
}
