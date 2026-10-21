//! USB removal notifications for idle hardware effects.
//!
//! Listen to kernel uevents, not the network or HID reports. The core owns
//! the active device identity and ignores removals of other USB devices.

use std::{
    io, mem,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    thread,
};

use crossbeam_channel::Sender;

use crate::core::Command;

/// USB uevents are small. A truncated event or socket overflow invalidates
/// the handle conservatively, since the missing event might be its removal.
const UEVENT_BUFFER_BYTES: usize = 8192;
const SOCKET_BUFFER_BYTES: libc::c_int = 256 * 1024;
const KERNEL_UEVENT_GROUP: u32 = 1;

pub fn spawn(command_tx: Sender<Command>) {
    // Subscribe before the core acquires the keyboard, so a removal cannot
    // slip between opening the device and starting the listener.
    let socket = match open_socket() {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("usb_watch: cannot watch controller removals: {error}");
            return;
        }
    };

    let spawn_result = thread::Builder::new()
        .name("usb-watch".to_string())
        .spawn(move || listen(socket, &command_tx));
    if let Err(error) = spawn_result {
        eprintln!("usb_watch: cannot start controller removal listener: {error}");
    }
}

fn open_socket() -> io::Result<OwnedFd> {
    // SAFETY: socket has no pointer arguments and returns a new owned fd.
    let raw_fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            libc::NETLINK_KOBJECT_UEVENT,
        )
    };
    if raw_fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: raw_fd is a newly opened descriptor with no other owner.
    let socket = unsafe { OwnedFd::from_raw_fd(raw_fd) };

    let receive_buffer_bytes = SOCKET_BUFFER_BYTES;
    // SAFETY: the value and its size describe a valid integer socket option.
    let result = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVBUF,
            std::ptr::addr_of!(receive_buffer_bytes).cast(),
            mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: sockaddr_nl is a plain C struct; zero initializes the reserved fields.
    let mut address: libc::sockaddr_nl = unsafe { mem::zeroed() };
    address.nl_family = libc::AF_NETLINK as libc::sa_family_t;
    address.nl_groups = KERNEL_UEVENT_GROUP;
    // SAFETY: address is valid for the supplied sockaddr_nl size.
    let result = unsafe {
        libc::bind(
            socket.as_raw_fd(),
            std::ptr::addr_of!(address).cast(),
            mem::size_of_val(&address) as libc::socklen_t,
        )
    };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(socket)
}

fn listen(socket: OwnedFd, command_tx: &Sender<Command>) {
    let mut buffer = [0u8; UEVENT_BUFFER_BYTES];
    loop {
        let command = match receive_removal(&socket, &mut buffer) {
            Ok(Some(command)) => command,
            Ok(None) => continue,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.raw_os_error() == Some(libc::ENOBUFS) => Command::UsbEventsLost,
            Err(error) => {
                eprintln!("usb_watch: controller removal listener stopped: {error}");
                return;
            }
        };
        // Only this listener can wait on a full core queue, never the core.
        // Backpressure preserves removals instead of silently losing recovery.
        if command_tx.send(command).is_err() {
            return;
        }
    }
}

fn receive_removal(socket: &OwnedFd, buffer: &mut [u8]) -> io::Result<Option<Command>> {
    // SAFETY: zero is a valid initial sockaddr_nl, filled by recvfrom below.
    let mut sender: libc::sockaddr_nl = unsafe { mem::zeroed() };
    let mut sender_bytes = mem::size_of_val(&sender) as libc::socklen_t;
    // SAFETY: both buffers are writable for their supplied sizes. MSG_TRUNC
    // returns the actual datagram length, which is checked before slicing.
    let received = unsafe {
        libc::recvfrom(
            socket.as_raw_fd(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            libc::MSG_TRUNC,
            std::ptr::addr_of_mut!(sender).cast(),
            &mut sender_bytes,
        )
    };
    if received < 0 {
        return Err(io::Error::last_os_error());
    }
    if sender_bytes as usize != mem::size_of_val(&sender) || sender.nl_pid != 0 {
        return Ok(None);
    }
    if received as usize > buffer.len() {
        return Ok(Some(Command::UsbEventsLost));
    }
    let command = removed_device(&buffer[..received as usize], sender.nl_pid).map(|device_name| {
        Command::KeyboardRemoved {
            device_name: device_name.to_string(),
        }
    });
    Ok(command)
}

fn removed_device(datagram: &[u8], sender_pid: u32) -> Option<&str> {
    // Netlink stamps the sender's port ID. Only port zero is the kernel.
    if sender_pid != 0 {
        return None;
    }
    let text = std::str::from_utf8(datagram).ok()?;
    if !text.ends_with('\0') {
        return None;
    }

    let mut action = None;
    let mut subsystem = None;
    let mut device_type = None;
    let mut device_path = None;
    for field in text.split('\0') {
        let Some((name, value)) = field.split_once('=') else {
            continue;
        };
        match name {
            "ACTION" => action = Some(value),
            "SUBSYSTEM" => subsystem = Some(value),
            "DEVTYPE" => device_type = Some(value),
            "DEVPATH" => device_path = Some(value),
            _ => {}
        }
    }

    if action != Some("remove") || subsystem != Some("usb") || device_type != Some("usb_device") {
        return None;
    }

    let device_path = device_path?.strip_prefix("/devices/")?;
    let device_name = device_path.rsplit('/').next()?;
    if device_name.is_empty() {
        return None;
    }
    Some(device_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REMOVED_CONTROLLER: &[u8] = concat!(
        "remove@/devices/usb5/5-1/5-1.3\0",
        "ACTION=remove\0",
        "DEVPATH=/devices/usb5/5-1/5-1.3\0",
        "SUBSYSTEM=usb\0",
        "DEVTYPE=usb_device\0",
        "PRODUCT=48d/c985/200\0",
    )
    .as_bytes();

    #[test]
    fn an_idle_controller_removal_is_reported() {
        assert_eq!(removed_device(REMOVED_CONTROLLER, 0), Some("5-1.3"));
    }

    #[test]
    fn userspace_cannot_inject_a_removal_notification() {
        assert_eq!(removed_device(REMOVED_CONTROLLER, 1000), None);
    }

    #[test]
    fn interface_events_and_other_subsystems_are_ignored() {
        for (original, replacement) in [
            ("DEVTYPE=usb_device", "DEVTYPE=usb_interface"),
            ("SUBSYSTEM=usb", "SUBSYSTEM=hid"),
            ("ACTION=remove", "ACTION=add"),
        ] {
            let datagram = String::from_utf8_lossy(REMOVED_CONTROLLER);
            let unrelated = datagram.replace(original, replacement);
            assert_eq!(removed_device(unrelated.as_bytes(), 0), None);
        }
    }

    #[test]
    fn incomplete_or_invalid_notifications_are_ignored() {
        for datagram in [
            &b""[..],
            &b"\xff\0"[..],
            &b"ACTION=remove\0DEVTYPE=usb_device\0"[..],
            &REMOVED_CONTROLLER[..REMOVED_CONTROLLER.len() - 1],
        ] {
            assert_eq!(removed_device(datagram, 0), None);
        }
    }
}
