# Changelog

All notable changes to Aurora are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Optional subsystems report their own state, so a feature that cannot
  work on a given machine says so instead of failing quietly. Fn+Space
  detection, the profile hotkey, and screen capture each report active,
  degraded, unavailable with a reason, or inactive. `aurora status`
  prints the ones a user can act on.
- Opt-in tracing behind the `AURORA_TRACE` environment variable. The
  daemon logs every ACPI event with its match verdict, every feature
  report with its payload and result, the slot counter read at
  acquisition, and a counter sample after each slot write. Off by
  default, and rate limited when on.
- `aurora slot` to show or select the active Fn+Space slot, and
  `aurora set --slot N` to write one slot without selecting it.
- A slot picker at the top of the app's Lighting page: four linked
  buttons, three slots plus off, the live one marked. Picking one selects
  it and applies it immediately, so slots are reachable without the
  keyboard shortcut. When Fn+Space detection is unavailable or degraded,
  a line under the buttons says so.
- An interface style guide (`docs/ui-style-guide.md`) covering hierarchy,
  the spacing scale, and what earns a place on screen, with the sources
  it draws on.
- A Project group on the Settings page linking to issue reporting,
  discussions, and the repository, with the version, license and author
  in a footer below it. Aurora is tested on one laptop, so the report
  link is how a failure on any other model reaches someone who can fix
  it, and that belongs on a page rather than inside a menu.

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
- The Lighting page now hides the settings an effect does not use rather
  than greying them out. Static shows no Speed or Direction, effects that
  ignore zone colours show no colour pickers, and the swipe wipe switch
  appears only in fill mode.
- The Daemon page is now Settings, and the app no longer says "daemon" to
  users who never asked for one. The group is Background Service, the
  disconnected page offers to start Aurora rather than a daemon, and a
  version mismatch is called that instead of an incompatible daemon.

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
- A single read error disabled Fn+Space detection for the life of the
  process. Read errors are now told apart: an interrupted read retries,
  dropped events report degraded rather than claiming everything is
  fine, and anything else reopens the socket on a bounded backoff.
- The keyboard preview showed stored zone colours under effects that
  ignore them, so a rainbow effect could be drawn as four static colours.
  It now shows nothing zone-coloured for those effects and darkness while
  the backlight is off.
- The preview split the keyboard into four equal bands when the real
  zones cover 24, 29, 25 and 18 keys, putting every boundary in the wrong
  place. Bands are now weighted by key count, so zone 4 is the numpad.
- Every show-or-hide decision in the app tested effective visibility,
  which is false whenever an ancestor is hidden. A group that should have
  been hidden while the window showed the disconnected view kept its own
  flag set and came back visible under an effect that does not use it.
- The ambient and swipe option groups were built visible and hidden only
  once daemon state arrived, so both flashed on screen at every startup.
- The app's connection worker kept reconnecting and delivering into a
  dead runtime after the window closed, because relm4's input sender
  reports send failures only to the log. It now stops when the component
  is gone.
- A daemon snapshot generated just before a local edit reached the daemon
  could overwrite the control being dragged. Local edits keep precedence
  for a short window; outside it the daemon is the source of truth.
- Tracing could write megabytes an hour while a software effect ran,
  because every frame logged a line. Successful writes are now limited
  to one line per second, carrying the count of writes since the last
  one. Failures are never suppressed.

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
