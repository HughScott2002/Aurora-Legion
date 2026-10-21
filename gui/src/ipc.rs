//! Daemon connection worker.
//!
//! One std thread owns the connect/reconnect loop and the socket reader;
//! a second thread drains the write queue. The GUI thread never blocks on
//! the socket: it hands requests to [`IpcHandle`] and receives
//! [`IpcUpdate`] messages through the relm4 sender.

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use aurora_protocol::ipc::{
    socket_path, DaemonState, ErrorKind, Event, Request, RequestEnvelope, Response, ServerMessage,
    MAX_LINE_BYTES, PROTOCOL_VERSION,
};
use crossbeam_channel::{Receiver, Sender};

/// Delays between reconnect attempts; the last entry repeats. Fast enough
/// that "start daemon → window comes alive" feels immediate.
const RECONNECT_BACKOFF: [Duration; 3] = [
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
];

/// Pending writes from the GUI. Bounded: if the daemon is wedged the GUI
/// drops updates instead of buffering forever (the last write wins anyway).
const WRITE_QUEUE_CAPACITY: usize = 64;
const MAX_PENDING_REPLIES: usize = 64;
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
const WRITER_CHECK_INTERVAL: Duration = Duration::from_millis(500);

type PendingReplies = Arc<Mutex<BTreeMap<u64, Instant>>>;

fn reply_expired(pending: &BTreeMap<u64, Instant>, now: Instant) -> bool {
    for sent_at in pending.values() {
        if now.saturating_duration_since(*sent_at) >= REPLY_TIMEOUT {
            return true;
        }
    }
    false
}

fn reply_wait(pending: &BTreeMap<u64, Instant>, now: Instant) -> Duration {
    let mut wait = REPLY_TIMEOUT;
    for sent_at in pending.values() {
        let remaining = (*sent_at + REPLY_TIMEOUT).saturating_duration_since(now);
        wait = wait.min(remaining);
    }
    wait.max(Duration::from_millis(1))
}

fn lock_pending(pending: &PendingReplies) -> std::sync::MutexGuard<'_, BTreeMap<u64, Instant>> {
    match pending.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// What the connection worker reports back to the GUI.
#[derive(Debug)]
pub enum IpcUpdate {
    Connected,
    Disconnected,
    State(Box<DaemonState>),
    RequestFailed {
        kind: ErrorKind,
        message: String,
    },
    ConnectionError(String),
    Unresponsive,
    /// The daemon speaks a protocol this build cannot talk to. Terminal:
    /// the worker stops reconnecting, because retrying cannot fix it.
    Incompatible(String),
}

/// GUI-side handle: fire-and-forget requests.
#[derive(Clone)]
pub struct IpcHandle {
    request_tx: Sender<Request>,
}

impl IpcHandle {
    pub fn send(&self, request: Request) -> bool {
        let send_result = self.request_tx.try_send(request);
        if send_result.is_err() {
            eprintln!("ipc: dropped request, write queue full or worker gone");
        }
        send_result.is_ok()
    }
}

/// Spawn the connection worker.
///
/// `deliver` forwards an update into the relm4 component and returns
/// whether it arrived. It returns false once the component is gone, which
/// is the worker's only way to learn the window closed: relm4's
/// `ComponentSender::input` swallows that error, so a worker built on it
/// keeps reconnecting and delivering into a dead runtime forever.
pub fn spawn<F>(deliver: F) -> IpcHandle
where
    F: Fn(IpcUpdate) -> bool + Send + Clone + 'static,
{
    let (request_tx, request_rx) = crossbeam_channel::bounded::<Request>(WRITE_QUEUE_CAPACITY);

    thread::spawn(move || {
        connection_loop(&request_rx, &deliver);
    });

    IpcHandle { request_tx }
}

fn connection_loop<F>(request_rx: &Receiver<Request>, deliver: &F)
where
    F: Fn(IpcUpdate) -> bool + Send + Clone + 'static,
{
    let mut attempt_count: usize = 0;

    loop {
        let path = socket_path();
        let connect_result = UnixStream::connect(&path);

        let stream = match connect_result {
            Ok(stream) => stream,
            Err(error) => {
                if attempt_count == 0 {
                    if !deliver(IpcUpdate::ConnectionError(error.to_string()))
                        || !deliver(IpcUpdate::Disconnected)
                    {
                        return;
                    }
                }
                let last_index = RECONNECT_BACKOFF.len() - 1;
                let delay = RECONNECT_BACKOFF[attempt_count.min(last_index)];
                attempt_count = (attempt_count + 1).min(last_index);
                thread::sleep(delay);
                continue;
            }
        };

        attempt_count = 0;
        if !deliver(IpcUpdate::Connected) {
            return;
        }

        let outcome = serve_connection(stream, request_rx, deliver);

        if !deliver(IpcUpdate::Disconnected) {
            return;
        }

        match outcome {
            // Reconnecting cannot change the daemon's protocol version.
            SessionOutcome::Incompatible => return,
            // The window is gone; nothing left to talk to.
            SessionOutcome::ClientGone => return,
            SessionOutcome::Closed => {}
        }
        thread::sleep(RECONNECT_BACKOFF[0]);
    }
}

/// Why a connection ended. Only a version mismatch stops the worker;
/// everything else is worth reconnecting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionOutcome {
    Closed,
    Incompatible,
    /// The component is gone, so there is nobody to deliver to.
    ClientGone,
}

/// Runs until the connection dies. Sends Subscribe + GetState first, then
/// pumps the write queue on this thread while a reader thread forwards
/// server lines.
fn serve_connection<F>(
    stream: UnixStream,
    request_rx: &Receiver<Request>,
    deliver: &F,
) -> SessionOutcome
where
    F: Fn(IpcUpdate) -> bool + Send + Clone + 'static,
{
    if stream.set_write_timeout(Some(REPLY_TIMEOUT)).is_err() {
        return SessionOutcome::Closed;
    }
    let pending: PendingReplies = Arc::new(Mutex::new(BTreeMap::new()));
    let pending_for_reader = pending.clone();
    let read_stream = match stream.try_clone() {
        Ok(clone) => clone,
        Err(error) => {
            eprintln!("ipc: could not clone stream: {error}");
            return SessionOutcome::Closed;
        }
    };

    let deliver_for_reader = deliver.clone();
    let reader_handle =
        thread::spawn(move || reader_loop(read_stream, &pending_for_reader, &deliver_for_reader));

    let mut writer = stream;
    let mut next_id: u64 = 1;

    let handshake = [
        Request::Hello {
            protocol_version: PROTOCOL_VERSION,
        },
        Request::Subscribe,
        Request::GetState,
    ];
    for request in handshake {
        if !write_request(&mut writer, &mut next_id, &request, &pending) {
            let _ = writer.shutdown(std::net::Shutdown::Both);
            return join_reader(reader_handle);
        }
    }

    loop {
        // A closed connection is noticed by the reader; the writer notices
        // on the next write. Poll with a timeout so a dead reader ends the
        // session even when the GUI sends nothing.
        match request_rx.recv_timeout(WRITER_CHECK_INTERVAL) {
            Ok(request) => {
                if !write_request(&mut writer, &mut next_id, &request, &pending) {
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
    join_reader(reader_handle)
}

fn join_reader(handle: thread::JoinHandle<SessionOutcome>) -> SessionOutcome {
    match handle.join() {
        Ok(outcome) => outcome,
        Err(_) => SessionOutcome::Closed,
    }
}

fn write_request(
    writer: &mut UnixStream,
    next_id: &mut u64,
    request: &Request,
    pending: &PendingReplies,
) -> bool {
    let envelope = RequestEnvelope {
        id: *next_id,
        req: request.clone(),
    };
    *next_id += 1;

    let serialized = match serde_json::to_string(&envelope) {
        Ok(serialized) => serialized,
        Err(error) => {
            eprintln!("ipc: could not serialize request: {error}");
            return false;
        }
    };

    {
        let mut pending = lock_pending(pending);
        if pending.len() >= MAX_PENDING_REPLIES {
            return false;
        }
        pending.insert(envelope.id, Instant::now());
    }
    let write_result = writeln!(writer, "{serialized}");
    write_result.is_ok()
}

fn reader_loop<F>(stream: UnixStream, pending: &PendingReplies, deliver: &F) -> SessionOutcome
where
    F: Fn(IpcUpdate) -> bool,
{
    let mut reader = BufReader::new(stream);
    let mut line = String::new();

    loop {
        let wait = {
            let pending = lock_pending(pending);
            let now = Instant::now();
            if reply_expired(&pending, now) {
                deliver(IpcUpdate::Unresponsive);
                return SessionOutcome::Closed;
            }
            reply_wait(&pending, now)
        };
        // The idle wake bounds a newly sent request's wait even if the
        // reader was already blocked. Idle subscriptions stay connected.
        if reader.get_ref().set_read_timeout(Some(wait)).is_err() {
            return SessionOutcome::Closed;
        }
        let remaining = MAX_LINE_BYTES.saturating_sub(line.len()) + 1;
        let read_result = (&mut reader).take(remaining as u64).read_line(&mut line);
        if line.len() > MAX_LINE_BYTES {
            return SessionOutcome::Closed;
        }
        match read_result {
            Ok(0) => return SessionOutcome::Closed, // Daemon closed the connection.
            Ok(bytes_read) => {
                if bytes_read > MAX_LINE_BYTES {
                    eprintln!("ipc: daemon sent an oversized line, disconnecting");
                    return SessionOutcome::Closed;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(error) => {
                deliver(IpcUpdate::ConnectionError(error.to_string()));
                eprintln!("ipc: read failed: {error}");
                return SessionOutcome::Closed;
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            continue;
        }

        let message: ServerMessage = match serde_json::from_str(trimmed) {
            Ok(message) => message,
            Err(error) => {
                deliver(IpcUpdate::ConnectionError(error.to_string()));
                return SessionOutcome::Closed;
            }
        };

        line.clear();
        match message {
            ServerMessage::Event(envelope) => {
                let Event::StateChanged { state } = envelope.event;
                if !deliver(IpcUpdate::State(Box::new(state))) {
                    return SessionOutcome::ClientGone;
                }
            }
            ServerMessage::Response(envelope) => {
                lock_pending(pending).remove(&envelope.id);
                match envelope.resp {
                    Response::State { state } => {
                        if !deliver(IpcUpdate::State(Box::new(state))) {
                            return SessionOutcome::ClientGone;
                        }
                    }
                    Response::Hello {
                        protocol_version,
                        daemon_version,
                    } => {
                        // A mismatch is terminal. v2 moved lighting into
                        // Profile::slots, so a mismatched peer would parse
                        // state it cannot represent and send requests the other
                        // side cannot honour. Staying connected and hoping is
                        // worse than saying so and stopping.
                        if protocol_version != PROTOCOL_VERSION {
                            deliver(IpcUpdate::Incompatible(format!(
                                "Daemon {daemon_version} speaks protocol v{protocol_version}, \
                                 this app speaks v{PROTOCOL_VERSION}. Update whichever is older."
                            )));
                            return SessionOutcome::Incompatible;
                        }
                    }
                    Response::Error { kind, message } => {
                        if !deliver(IpcUpdate::RequestFailed { kind, message }) {
                            return SessionOutcome::ClientGone;
                        }
                    }
                    Response::Ok => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unanswered_request_expires_even_if_the_socket_is_open() {
        let now = Instant::now();
        let mut pending = BTreeMap::new();
        pending.insert(4, now - REPLY_TIMEOUT);
        assert!(reply_expired(&pending, now));
    }

    #[test]
    fn an_idle_subscription_does_not_count_as_an_unanswered_request() {
        assert!(!reply_expired(&BTreeMap::new(), Instant::now()));
    }

    #[test]
    fn a_recent_request_keeps_its_reply_window() {
        let now = Instant::now();
        let mut pending = BTreeMap::new();
        pending.insert(4, now);
        assert!(!reply_expired(&pending, now));
    }

    #[test]
    fn reader_reports_a_stalled_reply_without_waiting_for_socket_close() {
        let (reader, _server) = UnixStream::pair().expect("socket pair");
        let pending: PendingReplies = Arc::new(Mutex::new(BTreeMap::new()));
        lock_pending(&pending).insert(1, Instant::now() - REPLY_TIMEOUT);
        let (updates_tx, updates_rx) = crossbeam_channel::bounded(1);
        let outcome = reader_loop(reader, &pending, &|update| updates_tx.send(update).is_ok());
        assert_eq!(outcome, SessionOutcome::Closed);
        assert!(matches!(updates_rx.try_recv(), Ok(IpcUpdate::Unresponsive)));
    }

    #[test]
    fn pending_reply_limit_rejects_more_work() {
        let (mut writer, _server) = UnixStream::pair().expect("socket pair");
        let pending: PendingReplies = Arc::new(Mutex::new(BTreeMap::new()));
        for id in 0..MAX_PENDING_REPLIES as u64 {
            lock_pending(&pending).insert(id, Instant::now());
        }
        let mut next_id = MAX_PENDING_REPLIES as u64;
        assert!(!write_request(
            &mut writer,
            &mut next_id,
            &Request::GetState,
            &pending,
        ));
        assert_eq!(lock_pending(&pending).len(), MAX_PENDING_REPLIES);
    }
}
