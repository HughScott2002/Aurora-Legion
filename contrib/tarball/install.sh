#!/usr/bin/env bash
# Install Aurora from the prebuilt tarball into the current user's home.
#
# Everything is user-level except the udev rule, which needs sudo and is
# asked about explicitly. Run from the unpacked tarball directory:
#
#   ./install.sh
#
# See README.txt in this directory for the runtime dependencies.

set -eu

HERE=$(cd "$(dirname "$0")" && pwd)

BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/scalable/apps"
UNIT_DIR="$HOME/.config/systemd/user"
UDEV_RULES="/etc/udev/rules.d/99-aurora.rules"

install_binaries() {
  install -Dm755 "$HERE/bin/aurora" "$BIN_DIR/aurora"
  install -Dm755 "$HERE/bin/aurora-gui" "$BIN_DIR/aurora-gui"
  echo "installed: $BIN_DIR/aurora and $BIN_DIR/aurora-gui"

  case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
      echo "note: $BIN_DIR is not on your PATH; add it to run 'aurora'" >&2
      echo "      from a terminal. The desktop entry and service work" >&2
      echo "      either way." >&2
      ;;
  esac
}

install_desktop_entry() {
  local desktop_src desktop_dst
  desktop_src="$HERE/share/applications/io.github.HughScott2002.Aurora.desktop"
  desktop_dst="$APP_DIR/io.github.HughScott2002.Aurora.desktop"

  mkdir -p "$APP_DIR"
  # Desktop Exec lines cannot use systemd-style %h, so write the
  # absolute path.
  sed "s|^Exec=aurora-gui$|Exec=$BIN_DIR/aurora-gui|" \
    "$desktop_src" >"$desktop_dst"

  install -Dm644 \
    "$HERE/share/icons/hicolor/scalable/apps/io.github.HughScott2002.Aurora.svg" \
    "$ICON_DIR/io.github.HughScott2002.Aurora.svg"

  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$APP_DIR" || true
  fi
  echo "installed: desktop entry and icon"
}

install_user_service() {
  local unit_dst
  unit_dst="$UNIT_DIR/aurora.service"

  mkdir -p "$UNIT_DIR"
  # %h expands to the home directory when systemd loads the unit.
  sed "s|^ExecStart=aurora daemon$|ExecStart=%h/.local/bin/aurora daemon|" \
    "$HERE/systemd/aurora.service" >"$unit_dst"

  if systemctl --user daemon-reload 2>/dev/null; then
    systemctl --user enable --now aurora.service
    echo "installed: systemd user service (enabled and started)"
  else
    echo "note: no systemd user session detected; enable the service" >&2
    echo "      later with: systemctl --user enable --now aurora" >&2
  fi
}

install_udev_rule() {
  if [ -f "$UDEV_RULES" ]; then
    echo "udev rule already present at $UDEV_RULES"
    return
  fi

  echo
  echo "The daemon needs a udev rule so your user can open the keyboard"
  echo "without root. This is the only step that uses sudo; it writes"
  echo "$UDEV_RULES and reloads udev."
  printf "Install it now? [y/N] "
  read -r answer
  if [ "$answer" = "y" ] || [ "$answer" = "Y" ]; then
    sudo install -Dm644 "$HERE/udev/99-aurora.rules" "$UDEV_RULES"
    sudo udevadm control --reload-rules
    sudo udevadm trigger
    echo "installed: $UDEV_RULES (replug the keyboard or reboot)"
  else
    echo "skipped. Install it later with:"
    echo "  sudo install -Dm644 $HERE/udev/99-aurora.rules $UDEV_RULES"
    echo "  sudo udevadm control --reload-rules && sudo udevadm trigger"
  fi
}

install_binaries
install_desktop_entry
install_user_service
install_udev_rule

echo
echo "Done. Check with: $BIN_DIR/aurora status"
