# Roadmap

Aurora is Linux-first. The goal is a keyboard tool that feels native on
Linux the way Ghostty feels native in a terminal: small, fast, at home
on the desktop it runs on. Windows support comes after that bar is met,
as a second native shell on the same core, never at Linux's expense.

Two rules hold across every milestone:

1. Linux is the flagship, not the port source. Nothing ships for
   Windows until Linux is excellent.
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

`aurora.exe` daemon + CLI running on real hardware. No GUI yet.

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
