# Runtime files

## Active paths

| Item | Path | Owner |
| --- | --- | --- |
| Control socket | `$XDG_RUNTIME_DIR/aurora.sock` | Daemon |
| Socket fallback | `/tmp/aurora.sock` | Daemon |
| Settings | `$XDG_CONFIG_HOME/aurora/settings.json` | Daemon |
| Default settings | `~/.config/aurora/settings.json` | Daemon |
| Invalid settings backup | `settings.json.invalid` beside the settings file | Daemon |
| Temporary settings write | `settings.json.tmp` beside the settings file | Daemon |
| AppImage daemon log | `~/.cache/aurora/appimage-daemon.log` | AppImage launcher |
| Manual user unit | `~/.config/systemd/user/aurora.service` | Installer or user |
| Manual udev rule | `/etc/udev/rules.d/99-aurora.rules` | Installer or administrator |

The daemon creates the socket when it starts and removes it on clean
shutdown. It treats a live socket as another running daemon.

The daemon alone reads and writes settings. It writes a sibling
temporary file, then renames it over the settings file. The GUI and CLI
use the socket.

## Packaged paths

Nix store paths vary. The package contains:

| Item | Relative package path |
| --- | --- |
| CLI and daemon | `bin/aurora` |
| GUI | `bin/aurora-gui` |
| User unit | `lib/systemd/user/aurora.service` |
| Desktop entry | `share/applications/io.github.HughScott2002.Aurora.desktop` |
| Icon | `share/icons/hicolor/scalable/apps/io.github.HughScott2002.Aurora.svg` |

The NixOS module embeds `udev/99-aurora.rules` from the source tree.

## Legacy settings migration

When the current settings file does not exist, Aurora checks these
locations in order:

1. `$XDG_CONFIG_HOME/legion-kb-rgb/settings.json`
2. The path in `$LEGION_KEYBOARD_CONFIG`
3. `./settings.json` in the daemon's working directory

Aurora copies the first match into the current settings path. It never
writes back to a legacy path.

## Protocol limits

| Limit | Value |
| --- | --- |
| JSON line length | 1 MiB |
| Core command queue | 64 commands |
| Subscriber outbound queue | 64 lines |
| Custom effect steps | 4096 |
| Settings save delay | 2 seconds after the last change |

See the [IPC protocol](../protocol.md) for framing and error behavior.
