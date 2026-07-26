# Troubleshoot Aurora

Start with:

```console
$ aurora status
```

Then use the section that matches its output.

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

The GUI preview and color pickers may lag behind a slot broadcast. The
keyboard and `aurora status` are the reliable state until
[issue #14](https://github.com/HughScott2002/Aurora-Legion/issues/14)
is resolved.

## Controller state survives reboot

The ITE controller can retain broken lighting outside the operating
system. A normal reboot may not clear it. Shut the laptop down and use
Lenovo's model-specific EC reset or NOVO procedure.

Read the [hardware evidence](../research/ite8295-hardware-profiles.md)
before treating this as an Aurora settings problem.
