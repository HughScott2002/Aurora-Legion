# Quick start

From clone to lights in a few minutes. Nix with flakes enabled is the
primary toolchain; a verified Ubuntu 24.04 path and a prebuilt tarball
are documented in [Without nix](#without-nix).

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

## Without nix

### AppImage

The lowest-effort path on x86_64 distros from 2024 onward (glibc 2.39
or newer): download the latest
`Aurora-<version>-x86_64.AppImage` from the
[releases page](https://github.com/HughScott2002/Aurora-Legion/releases),
`chmod +x` it, and run it. GTK, libadwaita, and the other libraries are
bundled. With no arguments it starts the daemon (if none is running)
and opens the GUI; with arguments it acts as the CLI, so
`./Aurora-<version>-x86_64.AppImage status` works. You still need
[Keyboard access](#keyboard-access) below, and for the daemon to start
at login use the tarball install or a user unit whose `ExecStart`
points at the AppImage with the `daemon` argument.

### Prebuilt tarball

The fastest path on any recent x86_64 distro (glibc 2.39 or newer,
GTK 4.14 or newer): download `aurora-<version>-x86_64-linux-gnu.tar.gz`
from the [releases page](https://github.com/HughScott2002/Aurora-Legion/releases),
unpack it, and run `./install.sh`. The installer stays inside your home
directory except for the udev rule, which it asks about first. Runtime
dependencies are listed in the tarball's `README.txt`.

### Build from source on Ubuntu 24.04 (verified)

This list is verified by building in an `ubuntu:24.04` container; it is
the same list `contrib/build-tarball.sh` uses. Debian 13 should match.

```console
$ sudo apt install build-essential pkg-config cmake clang libclang-dev \
    git curl libgtk-4-dev libadwaita-1-dev libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev libvpx-dev libaom-dev libyuv-dev \
    libusb-1.0-0-dev libudev-dev libssl-dev libx11-dev libxi-dev \
    libxtst-dev libxcb1-dev libxcb-shm0-dev libxcb-randr0-dev \
    libdbus-1-dev
$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- \
    --default-toolchain 1.94.0
$ export CXXFLAGS="-include cstdint"   # webm-sys needs it
$ cargo build --release --workspace --features aurora/scrap-pkg-config
```

Ubuntu's `libyuv-dev` ships no pkg-config file, so the build stops with
"The system library `libyuv` was not found" unless you provide one:

```console
$ mkdir -p ~/.local/share/pkgconfig
$ cat > ~/.local/share/pkgconfig/libyuv.pc <<'EOF'
prefix=/usr
libdir=/usr/lib/x86_64-linux-gnu
includedir=/usr/include

Name: libyuv
Description: YUV scaling and conversion library
Version: 0
Libs: -L${libdir} -lyuv
Cflags: -I${includedir}
EOF
$ export PKG_CONFIG_PATH="$HOME/.local/share/pkgconfig:${PKG_CONFIG_PATH:-}"
```

Fedora 40+ and Arch equivalents (unverified, names may drift; corrections
welcome in [Discussions](https://github.com/HughScott2002/Aurora-Legion/discussions)):

| Ubuntu | Fedora | Arch |
| --- | --- | --- |
| `libgtk-4-dev libadwaita-1-dev` | `gtk4-devel libadwaita-devel` | `gtk4 libadwaita` |
| `libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev` | `gstreamer1-devel gstreamer1-plugins-base-devel` | `gstreamer gst-plugins-base` |
| `libvpx-dev libaom-dev libyuv-dev` | `libvpx-devel libaom-devel libyuv-devel` | `libvpx aom libyuv` |
| `libusb-1.0-0-dev libudev-dev libssl-dev` | `libusb1-devel systemd-devel openssl-devel` | `libusb systemd openssl` |
| `libx11-dev libxi-dev libxtst-dev libxcb1-dev libxcb-shm0-dev libxcb-randr0-dev libdbus-1-dev` | `libX11-devel libXi-devel libXtst-devel libxcb-devel dbus-devel` | `libx11 libxi libxtst libxcb dbus` |

### Install the build manually

```console
$ install -Dm755 target/release/aurora ~/.local/bin/aurora
$ install -Dm755 target/release/aurora-gui ~/.local/bin/aurora-gui
$ sed "s|^Exec=aurora-gui$|Exec=$HOME/.local/bin/aurora-gui|" \
    gui/data/io.github.HughScott2002.Aurora.desktop \
    > ~/.local/share/applications/io.github.HughScott2002.Aurora.desktop
$ install -Dm644 gui/data/icons/hicolor/scalable/apps/io.github.HughScott2002.Aurora.svg \
    ~/.local/share/icons/hicolor/scalable/apps/io.github.HughScott2002.Aurora.svg
$ sed "s|^ExecStart=aurora daemon$|ExecStart=%h/.local/bin/aurora daemon|" \
    systemd/aurora.service > ~/.config/systemd/user/aurora.service
$ systemctl --user daemon-reload && systemctl --user enable --now aurora
```

Then set up [Keyboard access](#keyboard-access) below and check with
`aurora status`.

## Keyboard access

`/dev/hidraw*` is root-only on most distros. On NixOS, enable the
module (see the README install section); elsewhere, install
[`udev/99-aurora.rules`](../udev/99-aurora.rules), which covers every
supported product id:

```console
$ sudo install -Dm644 udev/99-aurora.rules /etc/udev/rules.d/99-aurora.rules
$ sudo udevadm control --reload-rules && sudo udevadm trigger
```

Then replug the keyboard (or reboot).

## Where things live at runtime

| Thing | Path |
| --- | --- |
| Control socket | `$XDG_RUNTIME_DIR/aurora.sock` |
| Settings | `~/.config/aurora/settings.json` |
| systemd unit (nix package) | `<store path>/lib/systemd/user/aurora.service` |
| systemd unit (tarball/manual) | `~/.config/systemd/user/aurora.service` |
| udev rules (non-NixOS) | `/etc/udev/rules.d/99-aurora.rules` |
