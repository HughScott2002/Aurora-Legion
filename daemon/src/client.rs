//! Socket client used by the CLI subcommands (and anything else that wants
//! to talk to a running daemon synchronously).

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    time::Duration,
};

use aurora_protocol::ipc::{socket_path, Request, RequestEnvelope, Response, ServerMessage, MAX_LINE_BYTES};

/// A CLI request/response round trip should be instant on a local socket;
/// anything slower means a wedged daemon and the CLI should say so.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
}

#[derive(Debug)]
pub enum ClientError {
    Io(std::io::Error),
    Protocol(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Io(error) => write!(f, "io error talking to daemon: {error}"),
            ClientError::Protocol(message) => write!(f, "protocol error: {message}"),
        }
    }
}

impl Client {
    /// Connect to the daemon socket. `Err` means no (responsive) daemon.
    pub fn connect() -> Result<Self, ClientError> {
        let path = socket_path();
        let stream = UnixStream::connect(&path).map_err(ClientError::Io)?;

        stream.set_read_timeout(Some(REPLY_TIMEOUT)).map_err(ClientError::Io)?;
        stream.set_write_timeout(Some(REPLY_TIMEOUT)).map_err(ClientError::Io)?;

        let read_stream = stream.try_clone().map_err(ClientError::Io)?;

        Ok(Self {
            reader: BufReader::new(read_stream),
            writer: stream,
            next_id: 1,
        })
    }

    /// Send one request and wait for its response. Event lines that arrive
    /// in between (possible once subscribed) are skipped.
    pub fn request(&mut self, request: Request) -> Result<Response, ClientError> {
        let envelope_id = self.next_id;
        self.next_id += 1;

        let envelope = RequestEnvelope { id: envelope_id, req: request };
        let serialized = serde_json::to_string(&envelope).map_err(|error| ClientError::Protocol(error.to_string()))?;

        writeln!(self.writer, "{serialized}").map_err(ClientError::Io)?;

        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = self.reader.read_line(&mut line).map_err(ClientError::Io)?;
            if bytes_read == 0 {
                return Err(ClientError::Protocol("daemon closed the connection".to_string()));
            }
            if bytes_read > MAX_LINE_BYTES {
                return Err(ClientError::Protocol(format!("daemon sent a line over {MAX_LINE_BYTES} bytes")));
            }

            let message: ServerMessage = serde_json::from_str(line.trim()).map_err(|error| ClientError::Protocol(error.to_string()))?;

            match message {
                ServerMessage::Response(response_envelope) => {
                    if response_envelope.id != envelope_id {
                        return Err(ClientError::Protocol(format!(
                            "response id {} does not match request id {envelope_id}",
                            response_envelope.id
                        )));
                    }
                    return Ok(response_envelope.resp);
                }
                ServerMessage::Event(_) => {
                    // Not subscribed in CLI mode, but tolerate stray events.
                    continue;
                }
            }
        }
    }
}
