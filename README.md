<div align="center">

# Aurora for Legion

**No Lenovo Vantage on Linux? The usual alternative keeps a window running to keep animated effects alive.**

Aurora runs them quietly in the background, restores your profile at login, and gives you a polished native app with more ways to control your keyboard.

<p>
  <a href="#install-on-nixos"><img src="https://img.shields.io/badge/-Install-ff2740?style=for-the-badge" alt="Install" /></a>&nbsp;
  <a href="#cli"><img src="https://img.shields.io/badge/-CLI-37f558?style=for-the-badge" alt="CLI" /></a>&nbsp;
  <a href="#measured-not-claimed"><img src="https://img.shields.io/badge/-Measurements-3584e4?style=for-the-badge" alt="Measurements" /></a>&nbsp;
  <a href="https://github.com/HughScott2002/Aurora-Legion/discussions"><img src="https://img.shields.io/badge/-Discussions-e01b96?style=for-the-badge" alt="Discussions" /></a>
</p>

<p>
  <img src="https://img.shields.io/badge/Rust-1.94-B7410E?logo=rust&logoColor=white" alt="Rust 1.94" />
  <img src="https://img.shields.io/badge/GTK4-libadwaita-4A86CF?logo=gnome&logoColor=white" alt="GTK4 + libadwaita" />
  <img src="https://img.shields.io/badge/Nix-flake-5277C3?logo=nixos&logoColor=white" alt="Nix flake" />
  <img src="https://img.shields.io/badge/systemd-user_service-2d2d2d" alt="systemd user service" />
  <img src="https://img.shields.io/badge/license-GPL--3.0-blue" alt="GPL-3.0" />
</p>

</div>

<!-- Add phone demo here: start an animated effect, close the GUI, show it continuing, reopen the GUI, then change it again. -->

<div align="center">
  <img src="docs/screenshot.png" alt="aurora GTK4 interface" width="560"/>
</div>

Set an animated effect, close the window, and keep the animation. Open Aurora later to change it again.

## Install on NixOS

NixOS and Home Manager are supported today. Packages for other Linux distributions are planned.

Aurora supports 4-zone RGB keyboards across select 2020 to 2024 Legion, IdeaPad, and LOQ laptops. Check [`driver/src/lib.rs`](driver/src/lib.rs) for exact USB IDs.

```nix
# flake inputs
aurora.url = "github:HughScott2002/Aurora-Legion";

# home-manager: run the daemon at login
imports = [ aurora.homeModules.default ];
services.aurora.enable = true;

# nixos: let your user open the keyboard without root
imports = [ aurora.nixosModules.default ];
hardware.aurora.enable = true;
```

To try it without installing, start the daemon and then the GUI:

```console
$ nix run github:HughScott2002/Aurora-Legion#daemon &
$ nix run github:HughScott2002/Aurora-Legion
```

For keyboard permissions or building from a clone, see the [quick start](docs/quick-start.md).

## Why Aurora

Lenovo Vantage does not run on Linux. [L5P-Keyboard-RGB](https://github.com/4JX/L5P-Keyboard-RGB) made control possible through its reverse-engineered driver and effect engine, but its UI, tray, and software effects share one process.

On Wayland, that process cannot hide to the tray ([#181](https://github.com/4JX/L5P-Keyboard-RGB/issues/181)). Close it and animated effects stop.

Aurora preserves the hardware work while moving profiles and effects into a persistent daemon.

| Capability        | L5P-Keyboard-RGB                      | Aurora                                             |
| ----------------- | ------------------------------------- | -------------------------------------------------- |
| Lighting lifetime | Animated effects need the app process | Animated effects continue after the GUI closes     |
| Startup           | Started manually                      | systemd user service, profile restored at login    |
| UI                | egui, fixed 500×460 window            | Native GTK4/libadwaita, GNOME HIG                  |
| CLI               | Separate one-shot process             | Talks to shared daemon state                       |
| Integration       | CLI and custom-effect JSON            | CLI, JSON IPC, systemd, and Home Manager modules   |
| Settings          | `./settings.json` in the working dir  | XDG config, atomic writes, migrates old files      |
| Keyboard unplug   | Can panic an effect thread            | Detected, reacquired with backoff, shown in the UI |

## Measured, not claimed

Same machine, same Nix pipeline, release builds. PSS and CPU were sampled twice over 60-second windows. [See the methodology and raw data](docs/measurements.md).

"Resident" compares each project's long-running control process: L5P-Keyboard-RGB's GUI and Aurora's daemon. Aurora's GUI uses about `61 MiB` but only while open.

| Metric                  | L5P-Keyboard-RGB 0.20.8  | Aurora                     | Verdict                             |
| ----------------------- | ------------------------ | -------------------------- | ----------------------------------- |
| Resident memory, Static | 82.6 MiB                 | 10.2 MiB                   | ✅ 8× smaller                       |
| Resident memory, Swipe  | 82.3 MiB                 | 10.8 MiB                   | ✅ 8× smaller                       |
| Resident CPU, idle      | 0.10%                    | 0.04%                      | ✅ 2.5× lower                       |
| Resident CPU, Swipe     | 0.52%                    | 0.55% to 0.97%             | ⚠️ comparable, more variance        |
| Binaries on disk        | 26.6 MB                  | 8.4 MB daemon + 2.5 MB GUI | ✅ 2.4× smaller combined            |
| GUI while open          | is the resident 82.6 MiB | 61 MiB, exits on close     | ✅ lighter, and transient by design |

## How it works

The daemon starts on its own at login. The GUI and CLI are clients, not the resident process.

```mermaid
graph LR
    GUI["aurora-gui<br/>GTK4 + libadwaita"] -- "JSON over<br/>unix socket" --> D
    CLI["aurora<br/>set · status · cycle-profile"] -- "same socket" --> D
    D["aurora daemon<br/>effect engine · profiles · settings"] -- hidapi --> KB[("4-zone<br/>keyboard")]
    SD["systemd --user"] -. "starts at login" .-> D
```

The daemon owns state behind one command loop: one thread mutates state and everything else sends messages. Channels and queues are bounded, and driver failures cannot panic the engine.

The code follows [TigerStyle](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md), adapted to Rust.

## CLI

```console
$ aurora status
daemon:   running (v0.21.0)
keyboard: connected
profile:  gaming (Static effect)

$ aurora set -e Swipe -c 255,0,0,0,255,0,0,0,255,255,0,255 -s 3
profile applied        # keeps running after the CLI exits; it lives in the daemon

$ aurora cycle-profile   # bind this to a GNOME shortcut for Wayland-native switching
```

## Community

Questions, ideas, and show-and-tell are welcome in [Discussions](https://github.com/HughScott2002/Aurora-Legion/discussions). Pull requests are welcome too.

Start with [CONTRIBUTING.md](CONTRIBUTING.md); code rules live in [docs/style-guide.md](docs/style-guide.md).

New frontends connect through the [`protocol`](protocol/) crate over JSON on a Unix socket. A TUI, KDE, or web client can be added without changing the daemon.

## Credits

- Aurora builds on [4JX/L5P-Keyboard-RGB](https://github.com/4JX/L5P-Keyboard-RGB). Its reverse-engineered USB HID driver, effects, and years of device support made this rearchitecture possible. Both projects are GPL-3.0.
