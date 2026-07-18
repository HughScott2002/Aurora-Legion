//! The unix socket server. One reader thread and one writer thread per
//! client; both stay dumb — parsing and serializing only — while every
//! decision happens in the core.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::Path,
    thread,
};

use crossbeam_channel::{Receiver, Sender};
use legion_kb_protocol::ipc::{ErrorKind, EventEnvelope, RequestEnvelope, Response, ResponseEnvelope, MAX_LINE_BYTES};

use crate::core::{Command, Outbound};

/// Per-client outbound queue. Deep enough for bursts of state events; a
/// client that falls further behind than this gets dropped by the core.
const OUTBOUND_QUEUE_CAPACITY: usize = 64;

pub enum BindOutcome {
    Bound(UnixListener),
    AlreadyRunning,
    Failed(std::io::Error),
}

/// Bind the daemon socket. A connectable socket means another daemon runs;
/// a dead socket file is stale and gets replaced.
pub fn bind_socket(socket_path: &Path) -> BindOutcome {
    match UnixListener::bind(socket_path) {
        Ok(listener) => BindOutcome::Bound(listener),
        Err(bind_error) if bind_error.kind() == std::io::ErrorKind::AddrInUse => {
            let probe = UnixStream::connect(socket_path);
            match probe {
                Ok(_) => return BindOutcome::AlreadyRunning,
                // Only ConnectionRefused proves the socket is dead. Any
                // other error (permissions, interrupts) must not trigger an
                // unlink that could tear down a live daemon's socket.
                Err(probe_error) if probe_error.kind() == std::io::ErrorKind::ConnectionRefused => {}
                Err(probe_error) => return BindOutcome::Failed(probe_error),
            }

            eprintln!("server: removing stale socket {}", socket_path.display());
            let remove_result = std::fs::remove_file(socket_path);
            if let Err(remove_error) = remove_result {
                return BindOutcome::Failed(remove_error);
            }

            match UnixListener::bind(socket_path) {
                Ok(listener) => BindOutcome::Bound(listener),
                Err(rebind_error) => BindOutcome::Failed(rebind_error),
            }
        }
        Err(bind_error) => BindOutcome::Failed(bind_error),
    }
}

/// Accept loop; runs on its own thread until the listener errors out
/// (normally: never) or the process exits.
pub fn serve(listener: &UnixListener, command_tx: &Sender<Command>) {
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                spawn_client_threads(stream, command_tx.clone());
            }
            Err(error) => {
                eprintln!("server: accept failed: {error}");
                return;
            }
        }
    }
}

fn spawn_client_threads(stream: UnixStream, command_tx: Sender<Command>) {
    let (out_tx, out_rx) = crossbeam_channel::bounded::<Outbound>(OUTBOUND_QUEUE_CAPACITY);

    let write_stream = match stream.try_clone() {
        Ok(clone) => clone,
        Err(error) => {
            eprintln!("server: could not clone client stream: {error}");
            return;
        }
    };

    thread::spawn(move || {
        client_writer_loop(write_stream, &out_rx);
    });

    thread::spawn(move || {
        client_reader_loop(stream, &command_tx, &out_tx);
    });
}

fn client_reader_loop(stream: UnixStream, command_tx: &Sender<Command>, out_tx: &Sender<Outbound>) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    loop {
        line.clear();

        let read_result = reader.read_line(&mut line);
        match read_result {
            Ok(0) => return, // EOF, client closed the connection.
            Ok(bytes_read) => {
                if bytes_read > MAX_LINE_BYTES {
                    send_protocol_error(out_tx, 0, &format!("line exceeds {MAX_LINE_BYTES} bytes"));
                    return;
                }
            }
            Err(error) => {
                eprintln!("server: client read failed: {error}");
                return;
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let envelope: RequestEnvelope = match serde_json::from_str(trimmed) {
            Ok(envelope) => envelope,
            Err(parse_error) => {
                send_protocol_error(out_tx, 0, &format!("could not parse request: {parse_error}"));
                continue;
            }
        };

        let command = Command::Ipc {
            envelope_id: envelope.id,
            request: envelope.req,
            out_tx: out_tx.clone(),
        };
        let send_result = command_tx.send(command);
        if send_result.is_err() {
            // Core is gone; the daemon is shutting down.
            return;
        }
    }
}

fn client_writer_loop(stream: UnixStream, out_rx: &Receiver<Outbound>) {
    let mut writer = stream;

    for outbound in out_rx.iter() {
        let serialized = match &outbound {
            Outbound::Response(envelope) => serde_json::to_string::<ResponseEnvelope>(envelope),
            Outbound::Event(envelope) => serde_json::to_string::<EventEnvelope>(envelope),
        };

        let json = match serialized {
            Ok(json) => json,
            Err(error) => {
                eprintln!("server: could not serialize outbound message: {error}");
                continue;
            }
        };

        let write_result = writeln!(writer, "{json}");
        if write_result.is_err() {
            // Client closed; dropping the receiver unblocks nothing else —
            // the core prunes this connection on its next broadcast.
            return;
        }
    }
}

fn send_protocol_error(out_tx: &Sender<Outbound>, envelope_id: u64, message: &str) {
    let envelope = ResponseEnvelope {
        id: envelope_id,
        resp: Response::Error {
            kind: ErrorKind::InvalidRequest,
            message: message.to_string(),
        },
    };
    let _ = out_tx.try_send(Outbound::Response(envelope));
}
