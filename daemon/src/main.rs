mod cli;
mod client;
mod core;
mod engine;
mod hotkey;
mod keyboard;
mod server;
mod settings;

use std::sync::{atomic::AtomicBool, Arc};

use clap::{Parser, Subcommand};
use aurora_protocol::ipc::socket_path;

/// Commands from every source (IPC clients, hotkey) funnel into the core
/// through one bounded queue.
const COMMAND_QUEUE_CAPACITY: usize = 64;

#[derive(Parser)]
#[command(author, version, about = "Aurora: Legion keyboard lighting daemon and control CLI", name = "aurora")]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Run the daemon: applies the saved profile, serves the control socket.
    Daemon,

    #[command(flatten)]
    Client(cli::ClientCommand),
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    match cli.command {
        CliCommand::Daemon => {
            run_daemon();
            std::process::ExitCode::SUCCESS
        }
        CliCommand::Client(command) => {
            // Client commands only: piping output (`aurora list | head`)
            // must end the process quietly like any unix tool. The daemon
            // must NOT do this — it relies on ignored SIGPIPE to survive
            // clients that disconnect mid-write.
            restore_default_sigpipe_for_cli();
            cli::run(command)
        }
    }
}

fn restore_default_sigpipe_for_cli() {
    let register_result = unsafe {
        signal_hook::low_level::register(signal_hook::consts::SIGPIPE, || {
            let _ = signal_hook::low_level::emulate_default_handler(signal_hook::consts::SIGPIPE);
        })
    };
    if let Err(error) = register_result {
        eprintln!("aurora: could not restore SIGPIPE handling: {error}");
    }
}

fn run_daemon() {
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    register_shutdown_signals(&shutdown_flag);

    let path = socket_path();
    let listener = match server::bind_socket(&path) {
        server::BindOutcome::Bound(listener) => listener,
        server::BindOutcome::AlreadyRunning => {
            eprintln!("aurora: another daemon is already running on {}", path.display());
            std::process::exit(1);
        }
        server::BindOutcome::Failed(error) => {
            eprintln!("aurora: could not bind {}: {error}", path.display());
            std::process::exit(1);
        }
    };

    eprintln!("aurora: daemon v{} listening on {}", env!("CARGO_PKG_VERSION"), path.display());

    let (command_tx, command_rx) = crossbeam_channel::bounded(COMMAND_QUEUE_CAPACITY);

    spawn_signal_listener(command_tx.clone());
    hotkey::spawn(command_tx.clone());

    // Accept loop on its own thread; it lives for the whole process, so the
    // handle is deliberately not joined.
    let accept_command_tx = command_tx.clone();
    std::thread::spawn(move || {
        server::serve(&listener, &accept_command_tx);
    });

    // The core loop runs on the main thread until a signal or a Shutdown
    // request arrives.
    core::run(&command_rx, &shutdown_flag);

    let remove_result = std::fs::remove_file(&path);
    if let Err(error) = remove_result {
        eprintln!("aurora: could not remove socket {}: {error}", path.display());
    }

    eprintln!("aurora: daemon stopped");
}

/// Forward SIGTERM/SIGINT into the core's command queue so a blocked
/// `recv_timeout` wakes immediately; the atomic flag from
/// `register_shutdown_signals` stays as the backstop.
fn spawn_signal_listener(command_tx: crossbeam_channel::Sender<core::Command>) {
    let signals = signal_hook::iterator::Signals::new([signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT]);

    let mut signals = match signals {
        Ok(signals) => signals,
        Err(error) => {
            eprintln!("aurora: could not start signal listener: {error}");
            return;
        }
    };

    std::thread::spawn(move || {
        for _signal in signals.forever() {
            let send_result = command_tx.send(core::Command::ShutdownSignal);
            if send_result.is_err() {
                return; // Core is gone; the process is exiting anyway.
            }
        }
    });
}

fn register_shutdown_signals(shutdown_flag: &Arc<AtomicBool>) {
    let signals = [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT];

    // Registration order matters: the conditional shutdown must run before
    // the flag is set within one delivery, so the FIRST signal only sets the
    // flag and the SECOND signal (flag already true) force-exits a stuck
    // daemon.
    for signal in signals {
        let register_result = signal_hook::flag::register_conditional_shutdown(signal, 1, shutdown_flag.clone());
        if let Err(error) = register_result {
            eprintln!("aurora: could not register forced shutdown for signal {signal}: {error}");
        }

        let register_result = signal_hook::flag::register(signal, shutdown_flag.clone());
        if let Err(error) = register_result {
            eprintln!("aurora: could not register signal {signal}: {error}");
        }
    }
}
