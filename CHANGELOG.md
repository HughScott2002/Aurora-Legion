# Changelog

All notable changes to Aurora are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.22.0] - 2026-07-21

### Added

- AppImage release artifact: one bundled file that starts the daemon
  if needed and opens the GUI, or acts as the CLI when given
  arguments. Runs on x86_64 distros with glibc 2.39 or newer, no
  packages required.
- Assistant install guide (`docs/install-with-ai.md`); the README
  prompt is now a single line linking to it.

## [0.21.0] - 2026-07-21

First tagged release. Everything below is the state of the project at
the point versioning started.

### Added

- Persistent daemon that owns effects and profiles, started at login by
  a systemd user service and restored across sessions.
- Native GTK4/libadwaita app; animated effects keep running after the
  window closes.
- CLI (`aurora status`, `aurora set`, `aurora cycle-profile`) sharing
  daemon state over JSON IPC on a unix socket.
- NixOS module (udev keyboard access) and Home Manager module (daemon
  service).
- Prebuilt Ubuntu 24.04 tarball with a user-level installer, plus a
  verified non-nix source build path (`docs/quick-start.md`).
- Standalone udev rules file (`udev/99-aurora.rules`) covering all
  supported keyboards.
- Support for 4-zone RGB keyboards across select 2020 to 2024 Legion,
  IdeaPad, and LOQ laptops, via the driver from
  [4JX/L5P-Keyboard-RGB](https://github.com/4JX/L5P-Keyboard-RGB).

[Unreleased]: https://github.com/HughScott2002/Aurora-Legion/compare/v0.22.0...HEAD
[0.22.0]: https://github.com/HughScott2002/Aurora-Legion/compare/v0.21.0...v0.22.0
[0.21.0]: https://github.com/HughScott2002/Aurora-Legion/releases/tag/v0.21.0
