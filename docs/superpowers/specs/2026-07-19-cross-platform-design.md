# Cross-platform design: Windows support and Linux universality

Date: 2026-07-19
Status: approved (pending final review)

## Goal

Make Aurora work on Windows and almost any Linux distribution while
staying small, minimal, native-feeling and beautiful. Model: the
Ghostty architecture, a portable headless core with one fully native
shell per platform.

## Architecture

```
                 Linux shell: aurora-gui (relm4 + libadwaita), exists, unchanged
shared core
driver/protocol/ Windows shell: aurora-win (Tauri + WebView2, Fluent-styled), future
daemon+CLI
boundary: JSON-lines protocol over local socket, the "libghostty C ABI"
```

- The daemon is the only always-resident process; shells are transient,
  dumb protocol clients. Shell memory cost only exists while a window
  is open.
- All decisions happen in the daemon core (existing invariant). Shells
  render state and send commands.
- The protocol crate is the stable, language-agnostic contract, the
  equivalent of libghostty's C ABI. Aurora splits at a process boundary
  instead of Ghostty's link-time boundary because keyboard control
  needs no in-process latency, and the daemon must outlive the GUI.

## Decisions and rationale

| Decision | Choice | Why |
|---|---|---|
| Shell strategy | Native shell per OS (Ghostty way) | Max native feel and beauty on each platform; core stays portable |
| Windows shell | Tauri + WebView2 | WebView2 is a system component on Win 10/11 so downloads stay ~5-10 MB; Rust backend links `aurora-protocol` directly; Fluent-styled web UI has a high beauty ceiling; RAM cost is transient because the shell is not resident |
| Linux distribution | Portable daemon binary + Flatpak GUI | One old-glibc daemon binary covers any modern distro; Flathub is the canonical channel for libadwaita apps |
| Web configurator | Rejected | Hosted-site-to-localhost now hits Chrome Local Network Access permission prompts (Chrome 142 for fetch, Chrome 147 for WebSockets); an embedded local web UI (Syncthing model) avoids that but was declined in favor of the native Tauri shell. If revisited, use the embedded variant, not the hosted one |
| WASM target | Not pursued | WASM grants no hardware or daemon access; the browser would be the lever, and the web shell was rejected |

Tauri was rejected for the Linux shell in the 2026-07-18 rearchitecture
because of WebKitGTK. That objection is Linux-specific; Windows Tauri
uses WebView2 and does not inherit it.

## Core portability changes

1. **Transport.** One small module wrapping the local IPC endpoint:
   unix socket on Linux, named pipe `\\.\pipe\aurora` on Windows.
   Either the `interprocess` crate or a hand-rolled `#[cfg]` twin,
   whichever reads cleaner under TigerStyle. Server, client and GUI all
   go through it. The stale-socket recovery in `bind_socket` stays
   Linux-only; named pipes do not leave corpses.
2. **Signals.** `#[cfg(unix)]` keeps the current signal-hook setup.
   `#[cfg(windows)]` uses `SetConsoleCtrlHandler`, feeding the same
   `Command::ShutdownSignal` into the core queue. The SIGPIPE handling
   for CLI mode becomes unix-only.
3. **Cargo target gating.**
   - `hidapi`: `linux-static-libusb` under a Linux target table; the
     native Windows backend under a Windows table. The driver already
     carries the `#[cfg]` device-match branches (usage page on
     Windows).
   - `scrap`: the `wayland` feature is Linux-only. DXGI capture on
     Windows is rustdesk's primary platform, so ambient is expected to
     work; gate it out of the Windows build only if it fights.
   - `signal-hook`: unix-only dependency.
4. **Paths.** `socket_path()` gains a Windows arm (named pipe name).
   Config stays on `dirs` (`%APPDATA%\aurora` on Windows).
5. **Invariants unchanged.** Single core thread owns daemon state;
   `protocol/` stays UI-free; shells never touch the settings file.

## Protocol as public contract

- Write `docs/protocol.md`: every request, response and event, framing
  rules, `MAX_LINE_BYTES`, error kinds. Complete enough to implement a
  shell without reading the Rust types.
- Add a `Hello { protocol_version }` exchange so shells can negotiate.
  This is the `libghostty-vt` move: the boundary becomes a documented,
  versioned artifact.

## Linux universality (packaging only, no daemon changes)

The daemon binary is already init-agnostic: it binds a socket and runs
a loop, no systemd APIs. Other init systems need only service files.

- **Daemon + CLI**: release binary built in an old-glibc container,
  hidapi statically linked against libusb. One binary for any modern
  distro. Full static musl is not attempted; scrap and device_query
  need system libraries.
- **`install.sh` + `dist/`**: udev hidraw rules, the systemd user
  unit, and an XDG autostart `.desktop` fallback that covers every
  desktop regardless of init. OpenRC and runit scripts live in `dist/`
  as contribution space, not a maintenance promise.
- **GUI**: Flatpak on Flathub, talking to the host daemon socket via
  socket permission. The Nix flake remains the primary dev path.

## Windows deliverables (phased)

- **Phase A, core**: `aurora.exe` daemon + CLI compile and run.
  Transport swap, Ctrl handler, and `aurora autostart enable` writing
  the HKCU Run key (the same CLI verb drives systemctl or XDG autostart
  on Linux). No Windows Service mode; the Run key is enough.
- **Phase B, shell**: Tauri app. Rust side links `aurora-protocol`
  directly and speaks the named pipe; web side is Fluent-styled; tray
  via Tauri's built-in support.

## Sequencing

1. Core portability + protocol doc. Zero Linux behavior change,
   `nix build` stays green.
2. Linux packaging: release CI, `install.sh`, Flatpak manifest.
3. Windows daemon + CLI (Phase A).
4. Tauri shell (Phase B).

Each phase ships independently. Phases 3 and 4 need a Windows machine
or VM with real hardware; HID behavior is not testable in CI.

## Non-goals

- macOS (no Legion hardware runs it).
- Windows Service mode (Run key autostart suffices).
- 32-bit targets.
- Hosted web configurator (see decision table).

## Testing

- Protocol: round-trip serialization tests in `protocol/`.
- CI: build matrix (ubuntu + windows) once phase 3 lands; Linux builds
  must stay green from phase 1 onward.
- On-device: manual verification per phase on real hardware, per the
  measurement expectations in CONTRIBUTING.
