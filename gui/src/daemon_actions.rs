//! Daemon lifecycle actions: systemctl queries and calls, plus the
//! no-systemd fallback (ask the daemon to exit over the socket, wait, spawn
//! a new one). All of it runs on short-lived threads so the GTK main loop
//! never blocks; results come back as [`AppMsg`] values.

use std::{
    io::{Read, Write},
    os::{fd::OwnedFd, unix::net::UnixStream},
    path::PathBuf,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use aurora_protocol::ipc::socket_path;

use crate::app::AppMsg;

pub const UNIT_NAME: &str = "aurora.service";
const SERVICE_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const SERVICE_CHECK_INTERVAL: Duration = Duration::from_millis(20);
const MAX_SERVICE_OUTPUT_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceAction {
    Start,
    Restart,
    EnableAutostart,
    DisableAutostart,
}

impl ServiceAction {
    pub fn failure_title(self) -> &'static str {
        match self {
            Self::Start => "The lighting service could not be started.",
            Self::Restart => "The lighting service could not be restarted.",
            Self::EnableAutostart | Self::DisableAutostart => {
                "Start at Login could not be changed."
            }
        }
    }
}

fn run_bounded(command: &mut Command, timeout: Duration) -> Result<Output, String> {
    // Socket pairs let output reads share the deadline, including when a
    // descendant keeps an output descriptor open after the command exits.
    let (mut stdout_reader, stdout_writer) =
        UnixStream::pair().map_err(|error| error.to_string())?;
    let (mut stderr_reader, stderr_writer) =
        UnixStream::pair().map_err(|error| error.to_string())?;
    let spawn_result = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(OwnedFd::from(stdout_writer)))
        .stderr(Stdio::from(OwnedFd::from(stderr_writer)))
        .spawn();
    // Command retains its descriptors for reuse. Close our write ends so
    // the readers see EOF when the child closes its copies.
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = spawn_result.map_err(|error| error.to_string())?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "The service command did not finish within {} seconds.",
                timeout.as_secs()
            ));
        }
        // Child has no blocking wait with a deadline in std. This poll is
        // bounded and runs only on a background action thread.
        thread::sleep(SERVICE_CHECK_INTERVAL);
    };
    let stdout = read_service_output(&mut stdout_reader, deadline, MAX_SERVICE_OUTPUT_BYTES)?;
    let remaining = MAX_SERVICE_OUTPUT_BYTES - stdout.len();
    let stderr = read_service_output(&mut stderr_reader, deadline, remaining)?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn read_service_output(
    stream: &mut UnixStream,
    deadline: Instant,
    capacity: usize,
) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut buffer = [0; MAX_SERVICE_OUTPUT_BYTES];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("The service command took too long.".to_string());
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|error| error.to_string())?;
        let bytes_read = stream
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if bytes_read == 0 {
            return Ok(output);
        }
        if output.len() + bytes_read > capacity {
            return Err("The service command returned too much output.".to_string());
        }
        output.extend_from_slice(&buffer[..bytes_read]);
    }
}

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

pub fn set_autostart<F>(enable: bool, request_id: u64, deliver: F)
where
    F: Fn(AppMsg) + Send + 'static,
{
    thread::spawn(move || {
        let verb = if enable { "enable" } else { "disable" };
        let error = run_systemctl(&[verb, UNIT_NAME]);
        deliver(AppMsg::ServiceActionFinished {
            request_id,
            action: if enable {
                ServiceAction::EnableAutostart
            } else {
                ServiceAction::DisableAutostart
            },
            error,
        });
    });
}

/// Restart via systemd when the unit exists; otherwise ask the daemon to
/// exit over the socket, wait until its socket is gone, and spawn a fresh
/// `aurora daemon`.
pub fn restart_daemon<F>(unit_available: bool, request_id: u64, deliver: F)
where
    F: Fn(AppMsg) + Send + 'static,
{
    thread::spawn(move || {
        if unit_available {
            let error = run_systemctl(&["restart", UNIT_NAME]);
            deliver(AppMsg::ServiceActionFinished {
                request_id,
                action: ServiceAction::Restart,
                error,
            });
            return;
        }

        let error = restart_without_systemd();
        deliver(AppMsg::ServiceActionFinished {
            request_id,
            action: ServiceAction::Restart,
            error,
        });
    });
}

/// Start the daemon (status page button): systemd first, spawn fallback.
pub fn start_daemon<F>(unit_available: bool, request_id: u64, deliver: F)
where
    F: Fn(AppMsg) + Send + 'static,
{
    thread::spawn(move || {
        if unit_available {
            let error = run_systemctl(&["start", UNIT_NAME]);
            deliver(AppMsg::ServiceActionFinished {
                request_id,
                action: ServiceAction::Start,
                error,
            });
            return;
        }

        let error = spawn_daemon_process();
        deliver(AppMsg::ServiceActionFinished {
            request_id,
            action: ServiceAction::Start,
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

    let output = match run_bounded(&mut command, SERVICE_COMMAND_TIMEOUT) {
        Ok(output) => output,
        Err(error) => return Some(format!("could not run systemctl: {error}")),
    };

    if output.status.success() {
        return None;
    }

    let detail = String::from_utf8_lossy(&output.stderr);
    Some(format!(
        "systemctl --user {} exited with {}: {}",
        args.join(" "),
        output.status,
        detail.trim()
    ))
}

fn read_unit_fragment_path() -> Option<String> {
    let mut command = Command::new("systemctl");
    command.args(["--user", "show", "-p", "FragmentPath", "--value", UNIT_NAME]);
    let output = run_bounded(&mut command, SERVICE_COMMAND_TIMEOUT).ok()?;

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
    let mut command = Command::new("systemctl");
    command.args(["--user", "is-enabled", UNIT_NAME]);
    let output = run_bounded(&mut command, SERVICE_COMMAND_TIMEOUT);

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
    if let Err(error) = writer.set_write_timeout(Some(SHUTDOWN_WAIT_MAX)) {
        return Some(error.to_string());
    }
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

/// Prefer the `aurora` binary that ships next to this GUI binary (nix
/// store or target dir); fall back to $PATH.
fn find_daemon_binary() -> PathBuf {
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(dir) = current_exe.parent() {
            let sibling = dir.join("aurora");
            if sibling.is_file() {
                return sibling;
            }
        }
    }

    PathBuf::from("aurora")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_failure_details_are_preserved() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf 'service was refused' >&2; exit 7"]);
        let output = run_bounded(&mut command, Duration::from_secs(2)).expect("command completes");
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stderr, b"service was refused");
    }

    #[test]
    fn a_stalled_service_command_is_stopped_at_the_deadline() {
        let mut command = Command::new(std::env::current_exe().expect("test executable"));
        command.args(["--ignored", "--exact", "daemon_actions::tests::slow_child"]);
        let started = Instant::now();
        let result = run_bounded(&mut command, Duration::from_millis(50));
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn an_inherited_output_descriptor_cannot_hold_the_action_open() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 2 &"]);
        let started = Instant::now();
        assert!(run_bounded(&mut command, Duration::from_millis(50)).is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    #[ignore = "child process for the deadline test"]
    fn slow_child() {
        thread::sleep(Duration::from_secs(2));
    }
}
