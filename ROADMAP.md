# Roadmap

Aurora targets the platform where the gap is. On Windows, Legion
owners already have working options: Lenovo's own software and
[L5P-Keyboard-RGB](https://github.com/4JX/L5P-Keyboard-RGB), the
project Aurora forked from, both control these keyboards today. On
Linux there is no native-feeling option: lighting that dies with the
window, no Wayland story, no daemon. That gap is why Aurora exists,
and it is where the work goes first.

Windows support is planned, not dropped. It arrives as a second native
shell on the same core once the Linux experience is complete. Until
then Windows users lose nothing: L5P-Keyboard-RGB keeps working there.

Two rules hold across every milestone:

1. Serve the bottleneck first. Windows has adequate options today;
   Linux does not. Effort goes where users are actually stuck.
2. Later milestones must not regress earlier ones. `nix build` stays
   the pre-push gate, and the daemon's measured footprint
   ([~10 MiB resident](docs/measurements.md)) is a budget, not a brag.

Architecture decisions behind this plan live in the
[cross-platform design spec](docs/superpowers/specs/2026-07-19-cross-platform-design.md).

## M1: Linux flagship polish

Native feel complete on GNOME/Wayland. The GUI and daemon behave like a
first-party tool on the desktop they target.

- [ ] GUI polish pass against the GNOME HIG: spacing, typography,
      keyboard preview refinement, accessibility.
- [ ] Deliberate empty and error states: daemon not running, keyboard
      unplugged, permission missing. Every failure visible and
      recoverable from the UI.
- [ ] Wayland-native hotkeys via the XDG GlobalShortcuts portal,
      replacing evdev polling; works on any Wayland compositor.
- [ ] Ambient effect reliability on Wayland: survive portal
      re-authorization, degrade gracefully with clear UI state when
      capture is unavailable.
- [ ] Protocol contract: `docs/protocol.md` covering every request,
      response and event, plus a `Hello { protocol_version }`
      handshake. This is what makes third-party frontends (TUI, KDE)
      possible without daemon changes.

## M2: Linux everywhere

Any modern distro, any init system, without asking users to adopt Nix.
The daemon binary is already init-agnostic (it binds a socket and runs
a loop); this milestone is packaging and distribution.

- [ ] Portable release binary for daemon + CLI, built against an old
      glibc baseline, hidapi statically linked.
- [ ] `install.sh` plus `dist/`: udev hidraw rules, systemd user unit,
      and an XDG autostart fallback that covers every desktop
      regardless of init. OpenRC/runit scripts accepted in `dist/` as
      contributions.
- [ ] Flatpak for the GUI, published on Flathub: appstream metadata,
      icon, screenshots, socket permission to reach the host daemon.
- [ ] Release CI producing tagged artifacts for all of the above.

## M3: Portability groundwork

Invisible on Linux by design: zero behavior change, `nix build` stays
green. Prepares the core to compile for Windows.

- [ ] Transport module: unix socket on Linux, named pipe on Windows,
      one interface for server, client and GUI.
- [ ] Signal handling split: signal-hook stays unix-only; Windows gets
      a console Ctrl handler feeding the same shutdown command.
- [ ] Cargo target gating: hidapi backends, scrap's `wayland` feature,
      signal-hook, per-OS path arms.
- [ ] CI compile check for the Windows target so drift is caught early.

## M4: Windows core

`aurora.exe` daemon + CLI running on real hardware. No GUI yet. Until
this milestone lands, Windows users are best served by
L5P-Keyboard-RGB or Lenovo's software.

- [ ] Daemon binds the named pipe, serves the same protocol.
- [ ] `aurora autostart enable` writes the HKCU Run key (same CLI verb
      that drives systemctl/XDG autostart on Linux).
- [ ] Effects verified on a Windows machine with Legion hardware;
      ambient capture via DXGI or gated out if it fights.

## M5: Windows shell

A native-feeling Windows app on the same protocol.

- [ ] Tauri + WebView2 app, Fluent-styled, tray via Tauri's built-in
      support. Rust side links `aurora-protocol` directly.
- [ ] Installer and signed release artifacts.

## Non-goals

- macOS: no Legion hardware runs it.
- Windows Service mode: Run key autostart is enough.
- Hosted web configurator: rejected in the design spec; if a web UI is
  ever revisited, it should be embedded in the daemon and served from
  localhost, not hosted remotely.
