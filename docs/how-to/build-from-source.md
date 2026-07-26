# Build Aurora from source

Nix is the supported development toolchain. Ubuntu 24.04 is the
verified path without Nix.

## Build with Nix

```console
$ git clone https://github.com/HughScott2002/Aurora-Legion
$ cd Aurora-Legion
$ git config core.hooksPath hooks
$ nix develop
$ export CXXFLAGS="-include cstdint"
$ cargo build --workspace --features aurora/scrap-pkg-config
```

Run the daemon in one terminal:

```console
$ ./target/debug/aurora daemon
```

Use another terminal for the CLI and GUI:

```console
$ ./target/debug/aurora status
$ ./target/debug/aurora-gui
```

Only one daemon can own the keyboard. Stop an installed instance first:

```console
$ systemctl --user stop aurora
```

## Run the checks

Inside `nix develop`, with `CXXFLAGS` still set:

```console
$ cargo test --workspace --features aurora/scrap-pkg-config
$ nix build
```

`nix build` is the required pre-push gate.

## Build on Ubuntu 24.04

Install the build dependencies:

```console
$ sudo apt install build-essential pkg-config cmake clang libclang-dev \
    git curl libgtk-4-dev libadwaita-1-dev libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev libvpx-dev libaom-dev libyuv-dev \
    libusb-1.0-0-dev libudev-dev libssl-dev libx11-dev libxi-dev \
    libxtst-dev libxcb1-dev libxcb-shm0-dev libxcb-randr0-dev \
    libdbus-1-dev
$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- --default-toolchain 1.94.0
```

Ubuntu's `libyuv-dev` lacks a pkg-config file. Create one:

```console
$ mkdir -p "$HOME/.local/share/pkgconfig"
$ cat > "$HOME/.local/share/pkgconfig/libyuv.pc" <<'EOF'
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

Build the release binaries:

```console
$ export CXXFLAGS="-include cstdint"
$ cargo build --release --workspace --features aurora/scrap-pkg-config
```

## Install a source build

Install the binaries and desktop files under your home directory:

```console
$ install -Dm755 target/release/aurora "$HOME/.local/bin/aurora"
$ install -Dm755 target/release/aurora-gui "$HOME/.local/bin/aurora-gui"
$ mkdir -p "$HOME/.local/share/applications" "$HOME/.config/systemd/user"
$ sed "s|^Exec=aurora-gui$|Exec=$HOME/.local/bin/aurora-gui|" \
    gui/data/io.github.HughScott2002.Aurora.desktop \
    > "$HOME/.local/share/applications/io.github.HughScott2002.Aurora.desktop"
$ install -Dm644 \
    gui/data/icons/hicolor/scalable/apps/io.github.HughScott2002.Aurora.svg \
    "$HOME/.local/share/icons/hicolor/scalable/apps/io.github.HughScott2002.Aurora.svg"
$ sed "s|^ExecStart=aurora daemon$|ExecStart=%h/.local/bin/aurora daemon|" \
    systemd/aurora.service > "$HOME/.config/systemd/user/aurora.service"
$ systemctl --user daemon-reload
$ systemctl --user enable --now aurora
```

Then [grant keyboard access](install-linux.md#grant-keyboard-access) and
run `aurora status`.
