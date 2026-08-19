# Troubleshoot Aurora

Start with:

```console
$ aurora status
```

Then use the section that matches its output. `status` names any
optional subsystem that is not working, with the reason: Fn+Space
detection and screen capture each report for themselves rather than
failing silently.

For a stubborn fault, restart the daemon with tracing on:

```console
$ systemctl --user stop aurora
$ AURORA_TRACE=1 aurora daemon
```

Tracing logs every ACPI event with its match verdict, every write to the
controller with its payload and result, and the slot counter read at
acquisition. Leave it off the rest of the time; it exists for a specific
failure, not for normal running.

## Daemon not running

Inspect the user service:

```console
$ systemctl --user status aurora --no-pager
$ journalctl --user -u aurora -e
```

Start it:

```console
$ systemctl --user start aurora
```

For direct logs, stop the service and run the daemon in the foreground:

```console
$ systemctl --user stop aurora
$ aurora daemon
```

An AppImage-started daemon logs to
`~/.cache/aurora/appimage-daemon.log`.

## Keyboard not found or permission denied

Confirm the controller exists:

```console
$ lsusb -d 048d:
```

Aurora supports the product IDs listed in
[`driver/src/lib.rs`](../../driver/src/lib.rs). If the ID is supported,
inspect access to the raw USB node, which is what Aurora opens:

```console
$ udevadm info -q path -n /dev/bus/usb/BBB/DDD
$ getfacl -p /dev/bus/usb/BBB/DDD
```

Find `BBB/DDD` from the bus and device numbers `lsusb` printed. The
logged-in user needs an ACL entry granting read and write. Aurora talks
to the controller through libusb, so `/dev/hidraw*` permissions are not
what matters here even though the device is an HID one.

Reinstall the [udev rule](install-linux.md#grant-keyboard-access), then
replug the keyboard or reboot.

### Access worked, then stopped after an upgrade

`TAG+="uaccess"` is a dynamic ACL that systemd-logind applies when a
session activates. Restarting udevd drops it, which a distribution
upgrade or `nixos-rebuild switch` will do, and it is not restored until
the session activates again. Log out and back in.

If a machine needs access that survives a udevd restart without a
re-login, add a static rule beside the shipped one, replacing the
product ID with yours:

```
SUBSYSTEM=="usb", ATTR{idVendor}=="048d", ATTR{idProduct}=="c985", MODE="0660", GROUP="users"
```

This is a real loosening: every member of that group can then reach the
controller, rather than whoever is physically logged in. Prefer the
re-login unless you have a reason not to.

### Confirming access really comes from the udev rule

Two things routinely make a broken rule look like a working one.

Group ownership can mask the ACL. The node's default mode is 0664
(systemd's `50-udev-default.rules`), so if its group is one you belong
to, `group::rw-` grants access whether or not the `uaccess` ACL landed.
Point the group at root and the ACL becomes the only thing left:

```console
$ sudo chgrp root /dev/bus/usb/BBB/DDD
$ getfacl -p /dev/bus/usb/BBB/DDD
```

Change the group, not the mode. `chmod` recalculates the ACL mask and
can disable the entry you are trying to test.

An already-open descriptor survives a permission change. The daemon
holds the device open, and revoking access does not close it, so
lighting keeps working until the daemon reopens the device. Restart it
before believing a result:

```console
$ systemctl --user restart aurora
$ aurora status
```

`keyboard: connected` after a restart, with the group pointed away from
you, means the ACL is doing the work.

## Another process owns the keyboard

The HID interface allows one owner. Look for competing processes:

```console
$ pgrep -af 'aurora|L5P|OpenRGB'
```

Stop the installed daemon before testing another build:

```console
$ systemctl --user stop aurora
```

Also close L5P-Keyboard-RGB and OpenRGB.

## GUI does not open

Run it from a terminal:

```console
$ aurora-gui
```

For a tarball or source install, check for missing libraries:

```console
$ ldd "$HOME/.local/bin/aurora-gui" | grep 'not found'
```

The AppImage bundles these libraries. Report a failure there as a bug.

If the GUI closes when the daemon disconnects, restart the daemon and
open the GUI again. Graceful disconnect handling remains tracked in
[issue #14](https://github.com/HughScott2002/Aurora-Legion/issues/14).

A GUI that reports a version mismatch is talking to a daemon from a
different release. It stops rather than staying connected and misreading
state. Restart the daemon so both sides come from the same install.

## Fn+Space shows firmware RGB or darkness

Aurora applies only the last slot after Fn+Space input has been quiet
for 250 ms. During that window, firmware may briefly show its own
effect. The Aurora color should then replace it.

Check the daemon state and log:

```console
$ aurora status
$ journalctl --user -u aurora -f
```

One physical tap should produce one slot selection. If the final state
stays dark or shows firmware RGB, restart the daemon to read the
initial EC slot again, then retry.

If Fn+Space is not detected at all, `aurora status` says so with the
reason, and the app repeats it under the slot buttons. Select slots with
the buttons or `aurora slot` in the meantime; everything except the key
itself keeps working.

## Controller state survives reboot

The ITE controller can retain broken lighting outside the operating
system. A normal reboot may not clear it. Shut the laptop down and use
Lenovo's model-specific EC reset or NOVO procedure.

Read the [hardware evidence](../research/ite8295-hardware-profiles.md)
before treating this as an Aurora settings problem.
