# Mature daemon and native GUI references

Date: 2026-07-19

## Recommendation

Use [OpenTabletDriver](https://github.com/OpenTabletDriver/OpenTabletDriver) as
Aurora's primary architecture reference. It is the closest mature match: a
hardware-owning daemon, an optional console client, and separate native GUI
frontends for Windows (WPF), Linux (GTK), and macOS. The project explicitly
states that the daemon does the device work, the GUI is unnecessary, and saved
settings take effect when the daemon starts. It has 5,745 commits, 35 releases,
and a current [v0.6.7 release](https://github.com/OpenTabletDriver/OpenTabletDriver/releases/tag/v0.6.7)
from April 2026.

No single repository covers every Aurora goal. OpenTabletDriver does not publish
a first-party resident-memory measurement and its daemon uses .NET, so it is not
evidence for Aurora's size target. Use three complementary references:

- [LACT](https://github.com/ilya-zlobintsev/LACT) for the Rust implementation
  shape on Linux.
- [Transmission](https://github.com/transmission/transmission) for long-lived
  daemon protocol design and compatibility.
- [HandBrake](https://github.com/HandBrake/HandBrake) for one shared core behind
  independently maintained platform GUIs.

## Comparison

| Repository | What it proves | What Aurora should copy | Important mismatch |
| --- | --- | --- | --- |
| [OpenTabletDriver](https://github.com/OpenTabletDriver/OpenTabletDriver) | A device daemon can stay independent of optional, platform-specific native shells. Its source tree has separate daemon, console, GTK, WPF, and macOS UX projects. | Daemon owns hardware and persisted state; GUI and CLI remain replaceable clients; ship the daemon separately for headless use. | C# and .NET throughout; GTK3 and MonoMac are not Aurora's chosen Linux stack; no published daemon PSS baseline. |
| [LACT](https://github.com/ilya-zlobintsev/LACT) | A current Rust workspace can cleanly separate `daemon`, `client`, `schema`, `cli`, and `gui` crates. Its service is independent of the graphical session and has a headless package. | Use it as the nearest code-level reference for Rust, GTK4/libadwaita, Relm4, socket permissions, headless builds, and release-size settings. | Linux-only, root/system service, and much broader monitoring logic. It provides no Windows shell and no published resident-memory baseline. |
| [Transmission](https://github.com/transmission/transmission) | A headless daemon plus CLI, web, Qt, GTK, and native macOS applications can survive many releases around a documented remote contract. Version 4.1 added JSON-RPC 2.0 and the protocol exposes a semantic version plus a compatibility history. | Treat protocol evolution as product work: document versions, preserve old clients deliberately, keep CLI traffic inspectable, and test concurrent clients. | Its local GTK and macOS apps host their own sessions; only Qt can also act as a remote client. Windows uses Qt, which is not the Ghostty definition of platform-native. Its HTTP, authentication, and remote-access scope would be unnecessary for Aurora. |
| [HandBrake](https://github.com/HandBrake/HandBrake) | A large portable C core (`libhb`) can support separate Linux GTK, Windows C#/WPF, and Objective-C macOS frontends for years. The repository has 13,483 commits and 54 releases, with v1.11.2 released in June 2026. | Keep shared behavior UI-free and allow each shell to use its platform's normal interaction and packaging conventions. Its [WPF project](https://github.com/HandBrake/HandBrake/blob/master/win/CS/HandBrakeWPF/HandBrakeWPF.csproj) is a useful shipped Windows reference. | It is an in-process library architecture, not a small always-running daemon or IPC design. Its product and core are far larger than Aurora needs. |

LACT is especially close to Aurora's current Linux code. Its
[workspace](https://github.com/ilya-zlobintsev/LACT/blob/master/Cargo.toml)
separates protocol schema and client code from the daemon and GUI, and its
[API](https://github.com/ilya-zlobintsev/LACT/blob/master/docs/API.md) uses
newline-separated JSON over a Unix socket. The
[v0.7.4 release](https://github.com/ilya-zlobintsev/LACT/releases/tag/v0.7.4)
records the move of all GTK components to Relm4, while
[v0.9.0](https://github.com/ilya-zlobintsev/LACT/releases/tag/v0.9.0)
records the move to libadwaita. Its release profile strips symbols, uses size
optimization, one codegen unit, and LTO. LACT
[v0.9.1](https://github.com/ilya-zlobintsev/LACT/releases/tag/v0.9.1) was
released in June 2026 after 1,193 commits and 34 releases.

Transmission is the better protocol reference, but not the overall template.
Its [RPC specification](https://github.com/transmission/transmission/blob/main/docs/rpc-spec.md)
defines JSON-RPC framing, semantic protocol versions, deprecations, and breaking
changes. The [Qt client documentation](https://github.com/transmission/transmission/blob/main/qt/README.txt)
also makes the architectural caveat explicit: GTK and macOS run self-contained
sessions, while Qt can connect to a remote session. The project had 16,896
commits and 83 releases, with
[v4.1.3](https://github.com/transmission/transmission/releases/tag/4.1.3)
released in June 2026.

## What "native like Ghostty" means

[Ghostty's own definition](https://ghostty.org/docs/about) is stronger than
"uses an operating-system webview." It uses Swift with AppKit/SwiftUI on macOS,
Zig with GTK4 on Linux, native widgets, platform conventions, and a shared
UI-free core. Its Windows tracker still calls for a dedicated Windows GUI and
native widgets; in April 2026 maintainers
[explicitly rejected GTK and Qt](https://github.com/ghostty-org/ghostty/discussions/2563)
as the Windows frontend direction. Ghostty therefore defines the target, but it
is not evidence of a shipped Windows implementation.

The approved Aurora design currently proposes Tauri and WebView2 for Windows.
[Tauri's documentation](https://v2.tauri.app/start/prerequisites/) states that
it uses Edge WebView2 to render content. That can produce a small,
native-feeling Windows application, but its controls are HTML/CSS rendered by a
Chromium webview, not Ghostty-style platform-native widgets.

If "native like Ghostty" is a strict goal, replace the future Tauri shell goal
with a WPF or WinUI shell over the same named-pipe protocol. OpenTabletDriver and
HandBrake provide mature WPF examples. If Tauri remains the choice, describe
the goal as "native-feeling" rather than "fully native." The transport boundary
means this decision can wait until after the Windows daemon and CLI work.

## Goals to carry into Aurora

1. Preserve the current daemon/client split. The daemon alone owns hardware,
   state, and settings; every GUI remains optional and transient.
2. Treat Aurora's measured 10.9 MiB daemon PSS, 0.03 to 0.05 percent idle CPU,
   and 8.4 MB binary as regression baselines. The upstream projects do not
   provide a comparable first-party daemon measurement, so do not invent a
   target from them.
3. Keep the documented, versioned JSON-lines contract and concurrent-client
   behavior. Add compatibility policy only when a breaking version is actually
   needed.
4. Keep Linux on Relm4, GTK4, and libadwaita. If Windows is added, use a separate
   Windows-native shell when strict nativeness matters.
5. Release daemon/CLI artifacts independently from GUI artifacts, including a
   headless install path.

Do not copy OpenTabletDriver's plugin breadth, LACT's remote TCP and telemetry,
Transmission's HTTP security surface, or HandBrake's build complexity. Those
solve product requirements Aurora does not have.

OpenRGB was considered because it is mature, controls RGB hardware, and ships
on Windows and Linux. Its [build target](https://github.com/CalcProgrammer1/OpenRGB/blob/master/OpenRGB.pro)
combines Qt UI, CLI, device controllers, and network server in one application,
so it is a useful hardware compatibility source but a poor reference for
Aurora's small-daemon and native-shell goals.
