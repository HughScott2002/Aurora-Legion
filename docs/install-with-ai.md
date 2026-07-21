# Aurora install guide for AI assistants

You are an AI assistant installing Aurora on the user's machine because
they pasted a prompt linking here. Follow this document; it is the
canonical procedure.

Aurora (https://github.com/HughScott2002/Aurora-Legion) is a keyboard
RGB daemon, CLI, and GTK4 app for Lenovo Legion, IdeaPad, and LOQ
laptops with 4-zone RGB keyboards. The daemon owns the lighting;
the GUI and CLI are clients.

Ground rules:

- Ask the user before running anything with sudo. Sudo is only needed
  for one udev rule and, on some paths, distro packages.
- Outside the user's home directory, only
  `/etc/udev/rules.d/99-aurora.rules` may be created.
- Report failures honestly and use the troubleshooting section instead
  of improvising.

## 1. Detect the environment

- Distro and version: `/etc/os-release`.
- Architecture: `uname -m` must be `x86_64`; stop otherwise and tell
  the user only x86_64 builds exist today.
- Keyboard: `lsusb` should show vendor id `048d` with a product id in:
  `c955 c963 c965 c973 c975 c983 c984 c985 c993 c994 c995`.
  If there is an `048d` device with a different product id, stop and
  help the user open an issue with the "unsupported keyboard" template
  at the repo, including the `lsusb` line. If there is no `048d` device
  at all, this laptop is not supported; say so and stop.

## 2. Install the udev rule (every path needs it)

The daemon cannot open the keyboard without it. With the user's
permission:

```
curl -fsSL https://raw.githubusercontent.com/HughScott2002/Aurora-Legion/main/udev/99-aurora.rules -o /tmp/99-aurora.rules
sudo install -Dm644 /tmp/99-aurora.rules /etc/udev/rules.d/99-aurora.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
```

Then ask the user to replug the keyboard (internal keyboards: reboot
later works too; `udevadm trigger` often suffices).

## 3. Pick an install path

In order of preference:

1. **NixOS**: do not use the steps below. Follow the "Install on
   NixOS" section of the repo README (flake with Home Manager and
   NixOS modules).
2. **AppImage** (default for everyone else with glibc 2.39 or newer;
   check with `ldd --version`): download the latest
   `Aurora-<version>-x86_64.AppImage` from
   https://github.com/HughScott2002/Aurora-Legion/releases, save it
   somewhere permanent such as `~/.local/bin/`, and `chmod +x` it.
   Running it with no arguments starts the daemon if needed and opens
   the GUI; with arguments it acts as the CLI
   (`Aurora-x86_64.AppImage status`). GTK and the other libraries are
   bundled; nothing else to install. On older glibc, fall back to the
   source build path.
3. **Tarball** (best for a permanent, native-feeling install; needs
   glibc 2.39+ and GTK 4.14+, i.e. Ubuntu 24.04, Debian 13,
   Fedora 40+, Arch): download
   `aurora-<version>-x86_64-linux-gnu.tar.gz` from the same releases
   page, install the runtime packages listed in its `README.txt` for
   the detected distro, unpack, and run its `install.sh`. It installs
   into `~/.local` and sets up the systemd user service.
4. **Source build**: follow the "Without nix" section of
   `docs/quick-start.md` in the repo (verified Ubuntu 24.04 package
   list, rustup 1.94.0, `CXXFLAGS="-include cstdint"`, cargo feature
   `aurora/scrap-pkg-config`).

Optional AppImage integration, if the user wants Aurora at login and
in the app grid (replace the path with the real AppImage location):

- Desktop entry `~/.local/share/applications/aurora.desktop` with
  `Exec=/home/USER/.local/bin/Aurora-x86_64.AppImage`, `Name=Aurora`,
  `Type=Application`, `Categories=Settings;`.
- systemd user unit `~/.config/systemd/user/aurora.service`:

  ```
  [Unit]
  Description=Aurora keyboard lighting daemon
  After=graphical-session.target
  PartOf=graphical-session.target

  [Service]
  ExecStart=/home/USER/.local/bin/Aurora-x86_64.AppImage daemon
  Restart=on-failure
  RestartSec=2

  [Install]
  WantedBy=graphical-session.target
  ```

  then `systemctl --user daemon-reload && systemctl --user enable --now aurora`.

## 4. Verify

- `aurora status` (or `Aurora-x86_64.AppImage status`) must report the
  daemon running and the keyboard connected.
- Set something visible:
  `aurora set -e Static -c 255,0,0,0,255,0,0,0,255,255,0,255` and ask
  the user if the keyboard changed.
- Launch the GUI and confirm the window opens.

## 5. Troubleshooting

- **Keyboard permission denied or not connected**: confirm
  `/etc/udev/rules.d/99-aurora.rules` exists, rerun
  `sudo udevadm control --reload-rules && sudo udevadm trigger`,
  replug or reboot, and inspect ACLs with `getfacl /dev/hidraw*`
  (the logged-in user should have `rw`).
- **Daemon not running**: `systemctl --user status aurora` and
  `journalctl --user -u aurora -e` if installed as a service; the unit
  binds to `graphical-session.target`, so confirm that target is
  active. AppImage-started daemons log to
  `~/.cache/aurora/appimage-daemon.log`. As a last resort run
  `aurora daemon` in the foreground and read stderr.
- **Another process owns the keyboard**: only one process can.
  `systemctl --user stop aurora` before running a second daemon, and
  check for L5P-Keyboard-RGB or OpenRGB instances.
- **GUI fails to start (tarball/source installs)**: check missing
  libraries with `ldd ~/.local/bin/aurora-gui | grep "not found"` and
  install the matching distro packages. For the AppImage this should
  not happen; report it as a bug.

## 6. Report back

Tell the user what was installed and where, and how to uninstall:
remove the installed files (the tarball's `README.txt` lists them; for
the AppImage, the file itself plus any desktop entry and unit created
above) and `/etc/udev/rules.d/99-aurora.rules`.
