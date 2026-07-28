//! Fn+Space detection: a blocking listener on the ACPI netlink socket.
//!
//! Pressing Fn+Space makes the EC cycle its hardware lighting slot and
//! raise the Lenovo GameZone "light profile change" WMI event. No kernel
//! driver consumes the event, but the WMI core forwards every WMI event
//! to the ACPI generic-netlink multicast group, so a userspace listener
//! sees it as an `acpi_genl_event` with device class `"wmi"` and event
//! type 0xE600 (the DSDT's `Notify` value 0xE6 as packed by the kernel).
//! Sources and DSDT evidence: `docs/research/ite8295-hardware-profiles.md`.
//!
//! On any setup failure (no netlink permission, family missing, non-Lenovo
//! hardware) the listener logs and disables itself; the daemon carries on
//! without Fn+Space sync. There is deliberately no polling fallback: the
//! EC's counter moves in response to the daemon's own lighting writes
//! (without firing this event), so watching it outside an event window
//! reads write noise and loops.
//!
//! The netlink plumbing is hand-rolled against libc because the only
//! operations needed are: resolve the `acpi_event` family, join its
//! multicast group, and block on `recv`. Parsing is kept in pure functions
//! over byte slices so it is unit-testable without a socket.

use std::{io, mem, os::fd::RawFd, thread, time::Duration};

use aurora_protocol::ipc::{Subsystem, SubsystemState};
use crossbeam_channel::Sender;

use crate::core::Command;

// Generic netlink control protocol (linux/genetlink.h).
const GENL_ID_CTRL: u16 = 0x10;
const CTRL_CMD_GETFAMILY: u8 = 3;
const CTRL_ATTR_FAMILY_ID: u16 = 1;
const CTRL_ATTR_FAMILY_NAME: u16 = 2;
const CTRL_ATTR_MCAST_GROUPS: u16 = 7;
const CTRL_ATTR_MCAST_GRP_NAME: u16 = 1;
const CTRL_ATTR_MCAST_GRP_ID: u16 = 2;

// Netlink wire format (linux/netlink.h).
const NLM_F_REQUEST: u16 = 1;
const NLMSG_ERROR: u16 = 2;
const NLMSG_HEADER_BYTES: usize = 16;
const GENL_HEADER_BYTES: usize = 4;
const ATTR_HEADER_BYTES: usize = 4;
/// Attribute types carry flag bits (NLA_F_NESTED and friends) in their two
/// high bits; mask them off before comparing.
const NLA_TYPE_MASK: u16 = 0x3fff;

// The ACPI event family (drivers/acpi/event.c).
const ACPI_FAMILY_NAME: &str = "acpi_event";
const ACPI_MCAST_GROUP_NAME: &str = "acpi_mc_group";
const ACPI_GENL_CMD_EVENT: u8 = 1;
const ACPI_GENL_ATTR_EVENT: u16 = 1;

// struct acpi_genl_event { char device_class[20]; char bus_id[15]; u32 type; u32 data; }
const DEVICE_CLASS_BYTES: usize = 20;
const BUS_ID_BYTES: usize = 15;
/// Offset of the `type` field: class + bus id + one alignment pad byte.
const EVENT_TYPE_OFFSET: usize = DEVICE_CLASS_BYTES + BUS_ID_BYTES + 1;

/// Device class the WMI core stamps on forwarded WMI events.
const WMI_DEVICE_CLASS: &str = "wmi";

/// The WMI mapper ACPI device the GameZone events come from ("PNP0C14:01"
/// on the researched machines; the instance suffix varies, so only the
/// PNP id is matched).
const WMI_BUS_ID_PREFIX: &str = "PNP0C14";

/// Event types of the GameZone light-profile-change notification, DSDT
/// notify ID 0xE6. Observed raw as 0x00E6 on a 2023 Pro (kernel 6.12);
/// maniac103's 2021 trace saw it packed as 0xE600, so both encodings are
/// accepted. Each press also fires an unrelated 0xE8 event, which the
/// filter drops.
const LIGHT_PROFILE_EVENT_TYPES: [u32; 2] = [0xE6, 0xE600];

/// Netlink datagrams are small; the largest we read is the control
/// family's GETFAMILY answer (family attrs plus its ops list).
const RECV_BUFFER_BYTES: usize = 8192;

/// Delays between attempts to reopen the socket after a read failure that
/// is neither an interruption nor a dropped datagram. The last entry
/// repeats until the attempt budget runs out.
const REOPEN_BACKOFF: [Duration; 3] = [Duration::from_millis(500), Duration::from_secs(2), Duration::from_secs(5)];

/// How many times to reopen the socket before giving up and reporting the
/// subsystem unavailable. Bounded so a permanently broken socket cannot
/// spin forever.
const MAX_REOPEN_ATTEMPTS: u32 = 5;

pub fn spawn(command_tx: Sender<Command>) {
    thread::spawn(move || {
        run_listener(&command_tx);
    });
}

fn report(command_tx: &Sender<Command>, state: SubsystemState) -> bool {
    let command = Command::SubsystemStatus {
        subsystem: Subsystem::SlotSync,
        state,
    };
    command_tx.send(command).is_ok()
}

/// Why a listening session ended.
enum ListenOutcome {
    /// The core is gone; the daemon is shutting down.
    CoreGone,
    /// The socket failed in a way that needs a fresh one.
    SocketFailed(String),
}

fn run_listener(command_tx: &Sender<Command>) {
    let trace_enabled = legion_rgb_driver::trace_enabled();
    let mut reopen_attempt: u32 = 0;

    loop {
        let (socket, family_id) = match connect_acpi_events() {
            Ok(connected) => connected,
            Err(message) => {
                // Setup failure is not retryable: the family is missing, or
                // this is not Lenovo hardware. Say so once and stop.
                eprintln!("slot_watch: {message}; Fn+Space detection disabled");
                report(command_tx, SubsystemState::Unavailable { reason: message });
                return;
            }
        };

        report(command_tx, SubsystemState::Active);
        reopen_attempt = 0;

        match listen(&socket, family_id, command_tx, trace_enabled) {
            ListenOutcome::CoreGone => return,
            ListenOutcome::SocketFailed(message) => {
                reopen_attempt += 1;
                if reopen_attempt > MAX_REOPEN_ATTEMPTS {
                    let reason = format!("netlink socket kept failing ({message}) after {MAX_REOPEN_ATTEMPTS} attempts");
                    eprintln!("slot_watch: {reason}; Fn+Space detection disabled");
                    report(command_tx, SubsystemState::Unavailable { reason });
                    return;
                }

                let backoff_index = ((reopen_attempt - 1) as usize).min(REOPEN_BACKOFF.len() - 1);
                let delay = REOPEN_BACKOFF[backoff_index];
                eprintln!("slot_watch: {message}; reopening the socket in {delay:?} (attempt {reopen_attempt})");
                report(
                    command_tx,
                    SubsystemState::Degraded {
                        reason: format!("reconnecting to the ACPI event socket ({message})"),
                    },
                );
                thread::sleep(delay);
            }
        }
    }
}

/// Block on the socket, forwarding every Fn+Space event, until the socket
/// needs replacing.
///
/// Read errors are not all the same, and treating them the same is what
/// made a single failure disable Fn+Space for the life of the process:
///
/// - `EINTR` means a signal arrived. Nothing was lost; read again.
/// - `ENOBUFS` means the kernel dropped datagrams because this socket fell
///   behind. Events were lost, so the slot count may now be wrong. The
///   subsystem keeps running but reports degraded, because claiming Active
///   here would be claiming a slot number we cannot vouch for.
/// - Anything else needs a fresh socket.
fn listen(socket: &NetlinkSocket, family_id: u16, command_tx: &Sender<Command>, trace_enabled: bool) -> ListenOutcome {
    let mut buffer = vec![0u8; RECV_BUFFER_BYTES];
    let mut reported_dropped_events = false;

    loop {
        let received_bytes = match socket.recv(&mut buffer) {
            Ok(received_bytes) => received_bytes,
            Err(error) => {
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }

                if error.raw_os_error() == Some(libc::ENOBUFS) {
                    if !reported_dropped_events {
                        eprintln!("slot_watch: the kernel dropped events; the active slot may be out of step");
                        reported_dropped_events = true;
                        let sent = report(
                            command_tx,
                            SubsystemState::Degraded {
                                reason: "the kernel dropped Fn+Space events, so the active slot may be out of step".to_string(),
                            },
                        );
                        if !sent {
                            return ListenOutcome::CoreGone;
                        }
                    }
                    continue;
                }

                return ListenOutcome::SocketFailed(error.to_string());
            }
        };

        // Every ACPI event is inspected, and under tracing every one is
        // logged whether it matched or not. Without that, a missing
        // Fn+Space reaction cannot be told apart from a tap that never
        // reached the socket.
        let datagram = &buffer[..received_bytes];
        let events = parse_acpi_events(datagram, family_id);

        let mut matched = false;
        for event in &events {
            let is_match = is_light_profile_event(event);
            if is_match {
                matched = true;
            }
            if trace_enabled {
                let device_class = String::from_utf8_lossy(event.device_class);
                let bus_id = String::from_utf8_lossy(event.bus_id);
                let event_type = event.event_type;
                eprintln!("trace: acpi event class={device_class} bus={bus_id} type={event_type:#06x} match={is_match}");
            }
        }

        if !matched {
            continue;
        }

        // Do not time-debounce this event. Confirmed physical taps can
        // arrive 140 ms apart; dropping one leaves the firmware's own
        // slot profile visible instead of Aurora's logical slot.
        let send_result = command_tx.send(Command::HardwareSlotEvent);
        if send_result.is_err() {
            return ListenOutcome::CoreGone;
        }
    }
}

/// Open a generic-netlink socket, resolve the `acpi_event` family and join
/// its multicast group. Returns the socket and the family id events will
/// arrive under.
fn connect_acpi_events() -> Result<(NetlinkSocket, u16), String> {
    let socket = NetlinkSocket::open_generic()?;
    socket.bind()?;

    let query = build_family_query();
    socket.send_to_kernel(&query)?;

    let mut buffer = vec![0u8; RECV_BUFFER_BYTES];
    let received_bytes = socket
        .recv(&mut buffer)
        .map_err(|error| format!("could not read from netlink socket: {error}"))?;
    let family = match parse_family_reply(&buffer[..received_bytes]) {
        Some(family) => family,
        None => return Err(format!("netlink family '{ACPI_FAMILY_NAME}' not found")),
    };

    socket.join_multicast_group(family.multicast_group_id)?;
    Ok((socket, family.family_id))
}

// --- Wire format: building ----------------------------------------------

/// The GETFAMILY request for `acpi_event`: nlmsghdr + genlmsghdr + one
/// FAMILY_NAME attribute.
fn build_family_query() -> Vec<u8> {
    let name_bytes = ACPI_FAMILY_NAME.as_bytes();
    let name_with_nul_bytes = name_bytes.len() + 1;
    let attr_bytes = ATTR_HEADER_BYTES + name_with_nul_bytes;
    let message_bytes = NLMSG_HEADER_BYTES + GENL_HEADER_BYTES + align4(attr_bytes);

    let mut message = vec![0u8; message_bytes];

    // nlmsghdr: length, type, flags, sequence, port id (0 = kernel picks).
    write_u32_ne(&mut message, 0, message_bytes as u32);
    write_u16_ne(&mut message, 4, GENL_ID_CTRL);
    write_u16_ne(&mut message, 6, NLM_F_REQUEST);
    write_u32_ne(&mut message, 8, 1);
    write_u32_ne(&mut message, 12, 0);

    // genlmsghdr: command, version, reserved.
    message[NLMSG_HEADER_BYTES] = CTRL_CMD_GETFAMILY;
    message[NLMSG_HEADER_BYTES + 1] = 1;

    // nlattr: FAMILY_NAME with a nul-terminated string payload.
    let attr_offset = NLMSG_HEADER_BYTES + GENL_HEADER_BYTES;
    write_u16_ne(&mut message, attr_offset, attr_bytes as u16);
    write_u16_ne(&mut message, attr_offset + 2, CTRL_ATTR_FAMILY_NAME);
    let name_offset = attr_offset + ATTR_HEADER_BYTES;
    message[name_offset..name_offset + name_bytes.len()].copy_from_slice(name_bytes);
    // The trailing nul and the padding are already zero.

    message
}

// --- Wire format: parsing ------------------------------------------------

struct AcpiFamily {
    family_id: u16,
    multicast_group_id: u32,
}

/// One netlink message inside a datagram: its type and its payload (the
/// bytes after the 16-byte nlmsghdr).
struct NetlinkMessage<'a> {
    message_type: u16,
    payload: &'a [u8],
}

fn split_messages(datagram: &[u8]) -> Vec<NetlinkMessage<'_>> {
    let mut messages: Vec<NetlinkMessage<'_>> = Vec::new();
    let mut offset: usize = 0;

    loop {
        let Some(message_bytes) = read_u32_ne(datagram, offset) else {
            break;
        };
        let message_bytes = message_bytes as usize;
        if message_bytes < NLMSG_HEADER_BYTES {
            break;
        }

        let Some(message_type) = read_u16_ne(datagram, offset + 4) else {
            break;
        };
        let Some(payload) = datagram.get(offset + NLMSG_HEADER_BYTES..offset + message_bytes) else {
            break;
        };

        messages.push(NetlinkMessage { message_type, payload });

        offset += align4(message_bytes);
        if offset >= datagram.len() {
            break;
        }
    }

    messages
}

/// Walk a flat run of netlink attributes, returning (type, payload) pairs.
/// Nested attribute payloads are walked by calling this again on them.
fn walk_attributes(bytes: &[u8]) -> Vec<(u16, &[u8])> {
    let mut attributes: Vec<(u16, &[u8])> = Vec::new();
    let mut offset: usize = 0;

    loop {
        let Some(attr_bytes) = read_u16_ne(bytes, offset) else {
            break;
        };
        let attr_bytes = attr_bytes as usize;
        if attr_bytes < ATTR_HEADER_BYTES {
            break;
        }

        let Some(raw_type) = read_u16_ne(bytes, offset + 2) else {
            break;
        };
        let Some(payload) = bytes.get(offset + ATTR_HEADER_BYTES..offset + attr_bytes) else {
            break;
        };

        attributes.push((raw_type & NLA_TYPE_MASK, payload));

        offset += align4(attr_bytes);
        if offset >= bytes.len() {
            break;
        }
    }

    attributes
}

fn parse_family_reply(datagram: &[u8]) -> Option<AcpiFamily> {
    for message in split_messages(datagram) {
        if message.message_type == NLMSG_ERROR {
            return None;
        }
        if message.message_type != GENL_ID_CTRL {
            continue;
        }

        let attr_bytes = message.payload.get(GENL_HEADER_BYTES..)?;

        let mut family_id: Option<u16> = None;
        let mut multicast_group_id: Option<u32> = None;

        for (attr_type, attr_payload) in walk_attributes(attr_bytes) {
            if attr_type == CTRL_ATTR_FAMILY_ID {
                family_id = read_u16_ne(attr_payload, 0);
            }
            if attr_type == CTRL_ATTR_MCAST_GROUPS {
                // Each entry is one group: a nested attr whose type is an
                // index and whose payload holds NAME and ID attributes.
                for (_group_index, group_payload) in walk_attributes(attr_payload) {
                    let mut name_matches = false;
                    let mut candidate_id: Option<u32> = None;
                    for (group_attr_type, group_attr_payload) in walk_attributes(group_payload) {
                        if group_attr_type == CTRL_ATTR_MCAST_GRP_NAME {
                            name_matches = nul_terminated_matches(group_attr_payload, ACPI_MCAST_GROUP_NAME);
                        }
                        if group_attr_type == CTRL_ATTR_MCAST_GRP_ID {
                            candidate_id = read_u32_ne(group_attr_payload, 0);
                        }
                    }
                    if name_matches {
                        multicast_group_id = candidate_id;
                    }
                }
            }
        }

        if let (Some(family_id), Some(multicast_group_id)) = (family_id, multicast_group_id) {
            return Some(AcpiFamily { family_id, multicast_group_id });
        }
    }

    None
}

/// One `acpi_genl_event` lifted out of a datagram. The string fields borrow
/// the receive buffer and are already trimmed at their first nul.
struct AcpiEvent<'a> {
    device_class: &'a [u8],
    bus_id: &'a [u8],
    event_type: u32,
}

/// Every ACPI event carried by one datagram of the `acpi_event` family.
/// Malformed or truncated messages are skipped rather than reported: the
/// kernel is the peer here, and a short read is not worth a daemon error.
fn parse_acpi_events(datagram: &[u8], family_id: u16) -> Vec<AcpiEvent<'_>> {
    let mut events: Vec<AcpiEvent<'_>> = Vec::new();

    for message in split_messages(datagram) {
        if message.message_type != family_id {
            continue;
        }

        let Some(command) = message.payload.first() else {
            continue;
        };
        if *command != ACPI_GENL_CMD_EVENT {
            continue;
        }

        let Some(attr_bytes) = message.payload.get(GENL_HEADER_BYTES..) else {
            continue;
        };

        for (attr_type, attr_payload) in walk_attributes(attr_bytes) {
            if attr_type != ACPI_GENL_ATTR_EVENT {
                continue;
            }

            let Some(device_class) = attr_payload.get(..DEVICE_CLASS_BYTES) else {
                continue;
            };
            let Some(bus_id) = attr_payload.get(DEVICE_CLASS_BYTES..DEVICE_CLASS_BYTES + BUS_ID_BYTES) else {
                continue;
            };
            let Some(event_type) = read_u32_ne(attr_payload, EVENT_TYPE_OFFSET) else {
                continue;
            };

            events.push(AcpiEvent {
                device_class: nul_terminated_str(device_class),
                bus_id: nul_terminated_str(bus_id),
                event_type,
            });
        }
    }

    events
}

/// True when this event is the GameZone light-profile-change WMI
/// notification: device class `"wmi"`, the Lenovo WMI mapper device, and
/// one of the two observed encodings of DSDT notify ID 0xE6.
fn is_light_profile_event(event: &AcpiEvent<'_>) -> bool {
    if event.device_class != WMI_DEVICE_CLASS.as_bytes() {
        return false;
    }
    if !event.bus_id.starts_with(WMI_BUS_ID_PREFIX.as_bytes()) {
        return false;
    }

    LIGHT_PROFILE_EVENT_TYPES.contains(&event.event_type)
}

// --- Byte helpers --------------------------------------------------------

/// Netlink aligns message and attribute lengths to 4 bytes.
fn align4(length: usize) -> usize {
    (length + 3) & !3
}

fn read_u16_ne(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let slice = bytes.get(offset..end)?;
    let mut raw = [0u8; 2];
    raw.copy_from_slice(slice);
    Some(u16::from_ne_bytes(raw))
}

fn read_u32_ne(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let slice = bytes.get(offset..end)?;
    let mut raw = [0u8; 4];
    raw.copy_from_slice(slice);
    Some(u32::from_ne_bytes(raw))
}

fn write_u16_ne(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_ne_bytes());
}

fn write_u32_ne(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
}

/// Compare a fixed-size, nul-padded C string field against `expected`.
fn nul_terminated_matches(field: &[u8], expected: &str) -> bool {
    nul_terminated_str(field) == expected.as_bytes()
}

/// The bytes of a fixed-size, nul-padded C string field up to its first nul.
fn nul_terminated_str(field: &[u8]) -> &[u8] {
    let mut string_end = field.len();
    for (index, byte) in field.iter().enumerate() {
        if *byte == 0 {
            string_end = index;
            break;
        }
    }
    &field[..string_end]
}

// --- Socket wrapper ------------------------------------------------------

/// A blocking generic-netlink socket. Everything here is a thin, explicit
/// wrapper over libc; errors carry the OS error text.
struct NetlinkSocket {
    fd: RawFd,
}

impl NetlinkSocket {
    fn open_generic() -> Result<Self, String> {
        // SAFETY: plain socket(2) call; the fd is owned by the returned
        // struct and closed in Drop.
        let fd = unsafe { libc::socket(libc::AF_NETLINK, libc::SOCK_RAW | libc::SOCK_CLOEXEC, libc::NETLINK_GENERIC) };
        if fd < 0 {
            return Err(format!("could not open netlink socket: {}", io::Error::last_os_error()));
        }
        Ok(Self { fd })
    }

    fn bind(&self) -> Result<(), String> {
        // SAFETY: sockaddr_nl is plain-old-data; zeroed is its "any port,
        // no groups" state.
        let mut address: libc::sockaddr_nl = unsafe { mem::zeroed() };
        address.nl_family = libc::AF_NETLINK as libc::sa_family_t;

        // SAFETY: address is a valid sockaddr_nl for the fd's family.
        let bind_result = unsafe {
            libc::bind(
                self.fd,
                std::ptr::addr_of!(address).cast::<libc::sockaddr>(),
                mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
            )
        };
        if bind_result < 0 {
            return Err(format!("could not bind netlink socket: {}", io::Error::last_os_error()));
        }
        Ok(())
    }

    fn send_to_kernel(&self, bytes: &[u8]) -> Result<(), String> {
        // The socket is bound but not connected, so the kernel destination
        // (port id 0, no groups) must be spelled out per message.
        // SAFETY: sockaddr_nl is plain-old-data; zeroed means "the kernel".
        let mut destination: libc::sockaddr_nl = unsafe { mem::zeroed() };
        destination.nl_family = libc::AF_NETLINK as libc::sa_family_t;

        // SAFETY: the pointer and length come from one live slice, and the
        // destination is a valid sockaddr_nl for the fd's family.
        let sent = unsafe {
            libc::sendto(
                self.fd,
                bytes.as_ptr().cast(),
                bytes.len(),
                0,
                std::ptr::addr_of!(destination).cast::<libc::sockaddr>(),
                mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
            )
        };
        if sent < 0 {
            return Err(format!("could not send netlink request: {}", io::Error::last_os_error()));
        }
        Ok(())
    }

    /// The raw error is preserved rather than flattened to a string: the
    /// caller distinguishes EINTR, ENOBUFS and everything else, and a
    /// string cannot carry that.
    fn recv(&self, buffer: &mut [u8]) -> io::Result<usize> {
        // SAFETY: the pointer and length come from one live mutable slice.
        let received = unsafe { libc::recv(self.fd, buffer.as_mut_ptr().cast(), buffer.len(), 0) };
        if received < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(received as usize)
    }

    fn join_multicast_group(&self, group_id: u32) -> Result<(), String> {
        // SAFETY: setsockopt with a u32 payload, as NETLINK_ADD_MEMBERSHIP
        // expects.
        let result = unsafe {
            libc::setsockopt(
                self.fd,
                libc::SOL_NETLINK,
                libc::NETLINK_ADD_MEMBERSHIP,
                std::ptr::addr_of!(group_id).cast(),
                mem::size_of::<u32>() as libc::socklen_t,
            )
        };
        if result < 0 {
            return Err(format!("could not join the acpi event group: {}", io::Error::last_os_error()));
        }
        Ok(())
    }
}

impl Drop for NetlinkSocket {
    fn drop(&mut self) {
        // SAFETY: fd is owned by this struct and not used after drop.
        unsafe { libc::close(self.fd) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test-side builders mirroring the kernel's wire format.

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }

    fn push_attribute(bytes: &mut Vec<u8>, attr_type: u16, payload: &[u8]) {
        let attr_bytes = ATTR_HEADER_BYTES + payload.len();
        push_u16(bytes, attr_bytes as u16);
        push_u16(bytes, attr_type);
        bytes.extend_from_slice(payload);
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
    }

    fn wrap_in_message(message_type: u16, genl_command: u8, attr_bytes: &[u8]) -> Vec<u8> {
        let message_bytes = NLMSG_HEADER_BYTES + GENL_HEADER_BYTES + attr_bytes.len();
        let mut message: Vec<u8> = Vec::new();
        push_u32(&mut message, message_bytes as u32);
        push_u16(&mut message, message_type);
        push_u16(&mut message, 0);
        push_u32(&mut message, 7);
        push_u32(&mut message, 0);
        message.push(genl_command);
        message.push(1);
        push_u16(&mut message, 0);
        message.extend_from_slice(attr_bytes);
        message
    }

    /// The old single-call shape, kept so each test still reads as one
    /// question about one datagram.
    fn datagram_matches(datagram: &[u8], family_id: u16) -> bool {
        let events = parse_acpi_events(datagram, family_id);
        for event in &events {
            if is_light_profile_event(event) {
                return true;
            }
        }
        false
    }

    fn sample_acpi_event(device_class: &str, bus_id: &str, event_type: u32) -> Vec<u8> {
        let mut event = vec![0u8; DEVICE_CLASS_BYTES + BUS_ID_BYTES + 1 + 8];
        event[..device_class.len()].copy_from_slice(device_class.as_bytes());
        event[DEVICE_CLASS_BYTES..DEVICE_CLASS_BYTES + bus_id.len()].copy_from_slice(bus_id.as_bytes());
        event[EVENT_TYPE_OFFSET..EVENT_TYPE_OFFSET + 4].copy_from_slice(&event_type.to_ne_bytes());
        event
    }

    #[test]
    fn family_query_has_expected_layout() {
        let query = build_family_query();

        // 16 nlmsghdr + 4 genlmsghdr + align4(4 + len("acpi_event") + nul).
        assert_eq!(query.len(), 36);
        assert_eq!(read_u32_ne(&query, 0), Some(36));
        assert_eq!(read_u16_ne(&query, 4), Some(GENL_ID_CTRL));
        assert_eq!(read_u16_ne(&query, 6), Some(NLM_F_REQUEST));
        assert_eq!(query[16], CTRL_CMD_GETFAMILY);
        assert_eq!(read_u16_ne(&query, 22), Some(CTRL_ATTR_FAMILY_NAME));
        assert_eq!(&query[24..34], ACPI_FAMILY_NAME.as_bytes());
        assert_eq!(query[34], 0);
    }

    #[test]
    fn family_reply_parses_id_and_group() {
        let mut group_entry: Vec<u8> = Vec::new();
        push_attribute(&mut group_entry, CTRL_ATTR_MCAST_GRP_NAME, b"acpi_mc_group\0");
        let mut group_id_payload: Vec<u8> = Vec::new();
        push_u32(&mut group_id_payload, 9);
        push_attribute(&mut group_entry, CTRL_ATTR_MCAST_GRP_ID, &group_id_payload);

        let mut groups: Vec<u8> = Vec::new();
        push_attribute(&mut groups, 1, &group_entry);

        let mut attrs: Vec<u8> = Vec::new();
        let mut family_id_payload: Vec<u8> = Vec::new();
        push_u16(&mut family_id_payload, 24);
        push_attribute(&mut attrs, CTRL_ATTR_FAMILY_ID, &family_id_payload);
        push_attribute(&mut attrs, CTRL_ATTR_MCAST_GROUPS, &groups);

        let datagram = wrap_in_message(GENL_ID_CTRL, 1, &attrs);

        let family = parse_family_reply(&datagram).expect("family reply should parse");
        assert_eq!(family.family_id, 24);
        assert_eq!(family.multicast_group_id, 9);
    }

    #[test]
    fn family_reply_without_group_is_rejected() {
        let mut attrs: Vec<u8> = Vec::new();
        let mut family_id_payload: Vec<u8> = Vec::new();
        push_u16(&mut family_id_payload, 24);
        push_attribute(&mut attrs, CTRL_ATTR_FAMILY_ID, &family_id_payload);

        let datagram = wrap_in_message(GENL_ID_CTRL, 1, &attrs);
        assert!(parse_family_reply(&datagram).is_none());
    }

    #[test]
    fn light_profile_event_is_recognized_in_both_encodings() {
        // Raw notify ID, as a 2023 Pro reports it (bus instance :02 there).
        let mut raw_attrs: Vec<u8> = Vec::new();
        push_attribute(&mut raw_attrs, ACPI_GENL_ATTR_EVENT, &sample_acpi_event("wmi", "PNP0C14:02", 0xE6));
        let raw_datagram = wrap_in_message(24, ACPI_GENL_CMD_EVENT, &raw_attrs);
        assert!(datagram_matches(&raw_datagram, 24));

        // Packed form, as maniac103's 2021 trace reports it.
        let mut packed_attrs: Vec<u8> = Vec::new();
        push_attribute(&mut packed_attrs, ACPI_GENL_ATTR_EVENT, &sample_acpi_event("wmi", "PNP0C14:01", 0xE600));
        let packed_datagram = wrap_in_message(24, ACPI_GENL_CMD_EVENT, &packed_attrs);
        assert!(datagram_matches(&packed_datagram, 24));
    }

    #[test]
    fn other_events_are_ignored() {
        // Wrong device class.
        let mut battery_attrs: Vec<u8> = Vec::new();
        push_attribute(&mut battery_attrs, ACPI_GENL_ATTR_EVENT, &sample_acpi_event("battery", "PNP0C0A:00", 0xE6));
        let battery_datagram = wrap_in_message(24, ACPI_GENL_CMD_EVENT, &battery_attrs);
        assert!(!datagram_matches(&battery_datagram, 24));

        // Wrong bus id (a WMI event from some other mapper device).
        let mut other_wmi_attrs: Vec<u8> = Vec::new();
        push_attribute(&mut other_wmi_attrs, ACPI_GENL_ATTR_EVENT, &sample_acpi_event("wmi", "OTHER123:00", 0xE6));
        let other_wmi_datagram = wrap_in_message(24, ACPI_GENL_CMD_EVENT, &other_wmi_attrs);
        assert!(!datagram_matches(&other_wmi_datagram, 24));

        // Wrong event type: the 0xE8 event that accompanies every press,
        // and the thermal-mode hotkey.
        let mut companion_attrs: Vec<u8> = Vec::new();
        push_attribute(&mut companion_attrs, ACPI_GENL_ATTR_EVENT, &sample_acpi_event("wmi", "PNP0C14:02", 0xE8));
        let companion_datagram = wrap_in_message(24, ACPI_GENL_CMD_EVENT, &companion_attrs);
        assert!(!datagram_matches(&companion_datagram, 24));

        let mut thermal_attrs: Vec<u8> = Vec::new();
        push_attribute(&mut thermal_attrs, ACPI_GENL_ATTR_EVENT, &sample_acpi_event("wmi", "PNP0C14:01", 0xD000));
        let thermal_datagram = wrap_in_message(24, ACPI_GENL_CMD_EVENT, &thermal_attrs);
        assert!(!datagram_matches(&thermal_datagram, 24));

        // Wrong family id entirely.
        let mut wmi_attrs: Vec<u8> = Vec::new();
        push_attribute(&mut wmi_attrs, ACPI_GENL_ATTR_EVENT, &sample_acpi_event("wmi", "PNP0C14:01", 0xE6));
        let wrong_family_datagram = wrap_in_message(30, ACPI_GENL_CMD_EVENT, &wmi_attrs);
        assert!(!datagram_matches(&wrong_family_datagram, 24));
    }

    #[test]
    fn truncated_datagrams_do_not_panic() {
        let mut attrs: Vec<u8> = Vec::new();
        push_attribute(&mut attrs, ACPI_GENL_ATTR_EVENT, &sample_acpi_event("wmi", "PNP0C14:01", 0xE6));
        let datagram = wrap_in_message(24, ACPI_GENL_CMD_EVENT, &attrs);

        for cut in 0..datagram.len() {
            let truncated = &datagram[..cut];
            // Must never panic; the result value does not matter.
            let _ = datagram_matches(truncated, 24);
            let _ = parse_family_reply(truncated);
        }
    }
}
