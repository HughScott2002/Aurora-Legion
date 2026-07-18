//! Daemon connection worker.
//!
//! One std thread owns the connect/reconnect loop and the socket reader;
//! a second thread drains the write queue. The GUI thread never blocks on
//! the socket: it hands requests to [`IpcHandle`] and receives
//! [`IpcUpdate`] messages through the relm4 sender.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender};
use aurora_protocol::ipc::{socket_path, DaemonState, Event, Request, RequestEnvelope, Response, ServerMessage, MAX_LINE_BYTES};

/// Delays between reconnect attempts; the last entry repeats. Fast enough
/// that "start daemon → window comes alive" feels immediate.
const RECONNECT_BACKOFF: [Duration; 3] = [Duration::from_millis(500), Duration::from_secs(1), Duration::from_secs(2)];

/// Pending writes from the GUI. Bounded: if the daemon is wedged the GUI
/// drops updates instead of buffering forever (the last write wins anyway).
const WRITE_QUEUE_CAPACITY: usize = 64;

/// What the connection worker reports back to the GUI.
#[derive(Debug)]
pub enum IpcUpdate {
    Connected,
    Disconnected,
    State(Box<DaemonState>),
    RequestFailed(String),
}

/// GUI-side handle: fire-and-forget requests.
#[derive(Clone)]
pub struct IpcHandle {
    request_tx: Sender<Request>,
}

impl IpcHandle {
    pub fn send(&self, request: Request) {
        let send_result = self.request_tx.try_send(request);
        if send_result.is_err() {
            eprintln!("ipc: dropped request, write queue full or worker gone");
        }
    }
}

/// Spawn the connection worker. `deliver` forwards updates into the relm4
/// component (it is a cloned `ComponentSender::input` under the hood).
pub fn spawn<F>(deliver: F) -> IpcHandle
where
    F: Fn(IpcUpdate) + Send + Clone + 'static,
{
    let (request_tx, request_rx) = crossbeam_channel::bounded::<Request>(WRITE_QUEUE_CAPACITY);

    thread::spawn(move || {
        connection_loop(&request_rx, &deliver);
    });

    IpcHandle { request_tx }
}

fn connection_loop<F>(request_rx: &Receiver<Request>, deliver: &F)
where
    F: Fn(IpcUpdate) + Send + Clone + 'static,
{
    let mut attempt_count: usize = 0;

    loop {
        let path = socket_path();
        let connect_result = UnixStream::connect(&path);

        let stream = match connect_result {
            Ok(stream) => stream,
            Err(_) => {
                let last_index = RECONNECT_BACKOFF.len() - 1;
                let delay = RECONNECT_BACKOFF[attempt_count.min(last_index)];
                attempt_count += 1;
                thread::sleep(delay);
                continue;
            }
        };

        attempt_count = 0;
        deliver(IpcUpdate::Connected);

        serve_connection(stream, request_rx, deliver);

        deliver(IpcUpdate::Disconnected);
    }
}

/// Runs until the connection dies. Sends Subscribe + GetState first, then
/// pumps the write queue on this thread while a reader thread forwards
/// server lines.
fn serve_connection<F>(stream: UnixStream, request_rx: &Receiver<Request>, deliver: &F)
where
    F: Fn(IpcUpdate) + Send + Clone + 'static,
{
    let read_stream = match stream.try_clone() {
        Ok(clone) => clone,
        Err(error) => {
            eprintln!("ipc: could not clone stream: {error}");
            return;
        }
    };

    let deliver_for_reader = deliver.clone();
    let reader_handle = thread::spawn(move || {
        reader_loop(read_stream, &deliver_for_reader);
    });

    let mut writer = stream;
    let mut next_id: u64 = 1;

    let handshake = [Request::Subscribe, Request::GetState];
    for request in handshake {
        if !write_request(&mut writer, &mut next_id, &request) {
            let _ = reader_handle.join();
            return;
        }
    }

    loop {
        // A closed connection is noticed by the reader; the writer notices
        // on the next write. Poll with a timeout so a dead reader ends the
        // session even when the GUI sends nothing.
        match request_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(request) => {
                if !write_request(&mut writer, &mut next_id, &request) {
                    break;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if reader_handle.is_finished() {
                    break;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Closing our end unblocks the reader thread if it is still alive.
    let shutdown_result = writer.shutdown(std::net::Shutdown::Both);
    if shutdown_result.is_err() {
        // Already closed; nothing to do.
    }
    let _ = reader_handle.join();
}

fn write_request(writer: &mut UnixStream, next_id: &mut u64, request: &Request) -> bool {
    let envelope = RequestEnvelope {
        id: *next_id,
        req: request.clone(),
    };
    *next_id += 1;

    let serialized = match serde_json::to_string(&envelope) {
        Ok(serialized) => serialized,
        Err(error) => {
            eprintln!("ipc: could not serialize request: {error}");
            return true; // Bad request, but the connection is fine.
        }
    };

    let write_result = writeln!(writer, "{serialized}");
    write_result.is_ok()
}

fn reader_loop<F>(stream: UnixStream, deliver: &F)
where
    F: Fn(IpcUpdate),
{
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    loop {
        line.clear();

        let read_result = reader.read_line(&mut line);
        match read_result {
            Ok(0) => return, // Daemon closed the connection.
            Ok(bytes_read) => {
                if bytes_read > MAX_LINE_BYTES {
                    eprintln!("ipc: daemon sent an oversized line, disconnecting");
                    return;
                }
            }
            Err(error) => {
                eprintln!("ipc: read failed: {error}");
                return;
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let message: ServerMessage = match serde_json::from_str(trimmed) {
            Ok(message) => message,
            Err(error) => {
                eprintln!("ipc: could not parse server line: {error}");
                continue;
            }
        };

        match message {
            ServerMessage::Event(envelope) => {
                let Event::StateChanged { state } = envelope.event;
                deliver(IpcUpdate::State(Box::new(state)));
            }
            ServerMessage::Response(envelope) => match envelope.resp {
                Response::State { state } => deliver(IpcUpdate::State(Box::new(state))),
                Response::Error { kind, message } => {
                    deliver(IpcUpdate::RequestFailed(format!("{kind:?}: {message}")));
                }
                Response::Ok | Response::Profiles { .. } | Response::CustomEffects { .. } => {
                    // Fire-and-forget acknowledgements; state events carry
                    // everything the GUI renders.
                }
            },
        }
    }
}
