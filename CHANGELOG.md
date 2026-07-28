# Changelog

All notable changes to Aurora are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Profiles now own their slots. A profile holds one lighting
  configuration per Fn+Space slot, so switching slots moves between three
  looks that belong to the same profile. A new profile starts red, green,
  and blue. Saving a profile saves all three slots.
- Slots are selectable, not only observable. `aurora slot 2` and
  `aurora set --slot 2` reach a slot directly, and selecting one applies
  it immediately. Fn+Space and client selection share one apply path, so
  daemon state and the keyboard cannot disagree.
- The startup slot decision now splits by what each source actually
  knows. The controller decides lit versus off, because only it knows
  whether the backlight was turned off while the daemon was down. The
  stored selection decides which slot, because Aurora's own writes moved
  the controller's counter during the previous session. An unreadable or
  mid-transition counter decides nothing and the stored slot stands.
- The IPC protocol is version 2. Lighting moved from `Profile` into
  `Profile.slots`, so a v1 client would misread every profile. Clients
  now stop on a version mismatch instead of staying connected and
  parsing state they cannot represent.
- State broadcasts carry profile and custom effect summaries instead of
  full bodies, and `PlayCustomEffectByName` starts a stored effect
  without sending it back to the daemon that holds it.

### Fixed

- Settings could be erased by a daemon that failed to read them. A file
  that cannot be parsed now makes settings read-only for the session, so
  shutdown cannot overwrite it with defaults.
- Failed settings writes were reported as successes. `save` now returns
  its error, the pending change stays pending, and the reason reaches
  clients and `aurora status`.
- Editing lighting while the backlight was off applied to the keyboard
  but was stored nowhere, so the change vanished on the next restart.
  The off position now rejects edits and says why.
- A machine where the slot counter cannot be read no longer has its
  lighting replaced by slot 1's at every startup.
- The one megabyte line limit did not bound anything: the reader
  allocated the whole line before checking it, and the writer never
  checked at all, so an oversized broadcast disconnected every client
  and then did it again on reconnect.
- Effect comparison was discriminant-only everywhere, so changing
  ambient frames per second or swipe mode could be detected as no change
  at all. Equality is now structural, with `same_variant` for the effect
  selector that wants the old behavior.

### Migration

- v1 settings convert on first run. The v1 per-slot lighting becomes the
  live profile's slots, and each saved v1 profile becomes a profile whose
  three slots match, so activating it looks the way it did before. The
  active slot is recovered by matching the live lighting against the
  slots. The original file is copied to `settings.json.v1-backup` first,
  and the conversion does not write anything if that backup cannot be
  secured.
- Profile files written by earlier versions still load; a flat file is
  lifted into all three slots.

### Added

- Opt-in tracing behind the `AURORA_TRACE` environment variable. The
  daemon logs every ACPI event with its match verdict, every feature
  report with its payload and result, the slot counter read at
  acquisition, and a counter sample after each slot write. Off by
  default, because software effects write continuously.

## [0.23.0] - 2026-07-26

### Added

- Fn+Space hardware profile sync. Aurora remembers one profile for each
  of three lighting slots, with missing slots starting red, green, and
  blue. The off state stops writes until the next switch. Daemon state,
  `aurora status`, and the app show the active slot (#14).
- A Diátaxis documentation index with focused install, build,
  troubleshooting, Fn+Space, CLI, runtime, architecture, and hardware
  sync pages.

### Fixed

- Rapid Fn+Space taps now land on the final requested slot instead of
  skipping slots, going dark, or falling back to firmware RGB.
- Hardware profiles now reach the controller in one complete feature
  report, avoiding stale intermediate colors and effects.

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

[Unreleased]: https://github.com/HughScott2002/Aurora-Legion/compare/v0.23.0...HEAD
[0.23.0]: https://github.com/HughScott2002/Aurora-Legion/compare/v0.22.0...v0.23.0
[0.22.0]: https://github.com/HughScott2002/Aurora-Legion/compare/v0.21.0...v0.22.0
[0.21.0]: https://github.com/HughScott2002/Aurora-Legion/releases/tag/v0.21.0
