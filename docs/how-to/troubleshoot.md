# Troubleshoot Aurora

Start with:

```console
$ aurora status
```

Then use the section that matches its output. `status` names any
optional subsystem that is not working, with the reason: Fn+Space
detection, the profile hotkey, and screen capture each report for
themselves rather than failing silently.

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
inspect hidraw access:

```console
$ getfacl /dev/hidraw*
```

The logged-in user needs read and write access. Reinstall the
[udev rule](install-linux.md#grant-keyboard-access), then replug the
keyboard or reboot.

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
