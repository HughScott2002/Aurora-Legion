# Quick start

From clone to lights in a few minutes. Everything assumes nix with
flakes enabled; there is no other supported toolchain setup.

## Just run it

```console
$ nix run github:HughScott2002/Aurora-Legion#daemon &   # or from a clone: nix run .#daemon
$ nix run github:HughScott2002/Aurora-Legion            # the GTK app
```

If the daemon reports `permission denied` for the keyboard, your user
cannot open the hidraw device yet; see [Keyboard access](#keyboard-access).

## Hack on it

```console
$ git clone https://github.com/HughScott2002/Aurora-Legion && cd Aurora-Legion
$ git config core.hooksPath hooks     # build gate: nix build runs before every push
$ nix develop                         # toolchain + GTK4 + all native deps
$ export CXXFLAGS="-include cstdint"  # webm-sys needs it outside `nix build`
$ cargo build --workspace --features aurora/scrap-pkg-config
$ ./target/debug/aurora daemon &
$ ./target/debug/aurora status
$ ./target/debug/aurora-gui
```

Checks that must stay green:

```console
$ cargo test --workspace --features aurora/scrap-pkg-config
$ cargo clippy --workspace --features aurora/scrap-pkg-config
$ nix build                           # what the pre-push hook runs
```

The daemon logs to stderr; run it in the foreground while developing.
Only one process can own the keyboard: stop a system-installed daemon
first (`systemctl --user stop aurora`).

## Keyboard access

`/dev/hidraw*` is root-only on most distros. On NixOS, enable the
module (see the README install section); elsewhere, add a udev rule for
your keyboard's product id (vendor is always `048d`, ids are listed in
`driver/src/lib.rs`):

```
KERNEL=="hidraw*", SUBSYSTEMS=="usb", ATTRS{idVendor}=="048d", ATTRS{idProduct}=="c985", TAG+="uaccess"
```

Reload with `sudo udevadm control --reload-rules && sudo udevadm trigger`,
then replug (or reboot).

## Where things live at runtime

| Thing | Path |
| --- | --- |
| Control socket | `$XDG_RUNTIME_DIR/aurora.sock` |
| Settings | `~/.config/aurora/settings.json` |
| systemd unit (packaged) | `<store path>/lib/systemd/user/aurora.service` |
