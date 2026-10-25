#!/usr/bin/env bash
# Capture the real GTK4 rendering of every Aurora screen and dialog, as
# ground-truth PNGs for checking the Figma kit against.
#
# Usage:
#   labs/true-pixel/capture.sh [connected|connecting|all]
#
#   connected   the four pages, the menu popover, the About dialog, the
#               Save Profile dialog, and the backlight-off state
#   connecting  the disconnected status page (socket unreachable)
#   all         both, in that order (default)
#
# Why it is shaped this way, since the shape is the point:
#
#   Clean HOME, real bus. The app runs with a fake HOME so it renders
#   stock Adwaita dark, exactly as a default GNOME machine sees it. The
#   session bus stays real and read-only: a private dbus-run-session
#   hangs app startup (the app talks to systemd's user manager over the
#   session bus), and the real bus carries the live color-scheme and
#   accent colour, which are part of what the desktop actually shows.
#
#   settings.ini in the clean home pins the production window layout
#   (`:close`) and the stock font (Adwaita Sans 11). Without a session
#   to carry those, GTK renders minimize/maximize/close controls that
#   GNOME never shows, and falls back to whatever fontconfig picks for
#   "Sans".
#
#   Display :77. A display :99 belongs to someone else on this machine
#   and must not be driven by a capture script.
#
#   GSK_RENDERER=cairo: Xvfb has no GL, and a software raster is
#   deterministic between runs.
#
#   The window is forced to 640x720 at 0,0. With no window manager, GTK
#   maps the window 1x1 and waits for a configure that never comes, so
#   screen coordinates equal window coordinates from then on.
#
#   Every click below sits at a coordinate measured from a capture
#   (rowruns.sh is the measuring tool), not derived from CSS. Each
#   capture is followed by a pixel sample with the expected value, so a
#   moved widget fails loudly here instead of quietly in the Figma file.
#
# Outputs land in $WORK/out as 01-lighting.png through 09-connecting.png.
# The slot selection is saved before the run and restored after; the
# backlight-off capture does briefly turn the real keyboard off.

set -u

WORK="${WORK:-/tmp/aurora-true-pixel}"
DISPLAY_NUM="${DISPLAY_NUM:-77}"
export DISPLAY=":$DISPLAY_NUM"
XDTOOL="${XDTOOL:-$(command -v xdotool || true)}"

# Named bounds, per the repo rules: waits are counted, never infinite.
WINDOW_WAIT_ATTEMPTS=15
WINDOW_WAIT_SECONDS=2

HOME_DIR="$WORK/home"
OUT="$WORK/out"

# Click coordinates, measured from real captures of 0.25.0 at 640x720.
TAB_Y=23
TAB1_X=120
TAB2_X=240
TAB3_X=360
TAB4_X=480
MENU_X=572
ABOUT_ITEM_Y=99
PLUS_X=558
PLUS_Y=87
SLOT_Y=86
SLOT_OFF_X=234

missing=""
need() { command -v "$1" >/dev/null 2>&1 || missing="$missing $1"; }
need Xvfb
need import
need convert
need aurora
need aurora-gui
if [ -z "$XDTOOL" ]; then
    missing="$missing xdotool(run inside: nix shell nixpkgs#xdotool)"
fi
if [ -n "$missing" ]; then
    echo "missing commands:$missing" >&2
    exit 1
fi

# --- work directory and the clean home -------------------------------

mkdir -p "$HOME_DIR/.config/gtk-4.0" "$HOME_DIR/.cache" "$WORK/runtime"
chmod 700 "$WORK/runtime"
mkdir -p "$OUT"

settings_ini="$HOME_DIR/.config/gtk-4.0/settings.ini"
if [ ! -f "$settings_ini" ]; then
    cat > "$settings_ini" <<'EOF'
[Settings]
gtk-decoration-layout=:close
gtk-font-name=Adwaita Sans 11
gtk-icon-theme-name=Adwaita
EOF
fi

# A first run in an empty home builds fontconfig caches before the
# window appears, which otherwise eats the window-wait budget.
env HOME="$HOME_DIR" XDG_CACHE_HOME="$HOME_DIR/.cache" fc-cache -f >/dev/null 2>&1

# --- helpers ----------------------------------------------------------

failures=""

# Sample one pixel from a capture. The expected value is a substring of
# the srgb triple, so "56,56,59" matches antialiasing drift of a level
# or two but not a different surface.
check() {
    local label="$1" png="$2" x="$3" y="$4" expect="$5"
    local got
    got=$(convert "$OUT/$png" -depth 8 -format '%[pixel:p{'"$x"','"$y"'}]' info: 2>/dev/null)
    case "$got" in
        *"$expect"*) ;;
        *) failures="$failures
$label: expected *$expect* at ($x,$y), got $got" ;;
    esac
}

# The inverse: the pixel must NOT be the given surface. Used for
# dialogs, whose contents vary; "not the parent window background" is
# the one thing a failed dialog open can be counted on to be.
check_not() {
    local label="$1" png="$2" x="$3" y="$4" absent="$5"
    local got
    got=$(convert "$OUT/$png" -depth 8 -format '%[pixel:p{'"$x"','"$y"'}]' info: 2>/dev/null)
    case "$got" in
        *"$absent"*) failures="$failures
$label: found $absent at ($x,$y), expected dialog content" ;;
    esac
}

click() {
    "$XDTOOL" mousemove "$1" "$2"
    "$XDTOOL" click 1
    sleep 0.8
}

# The app window sits at 0,0 over the whole crop, so a root capture
# cropped to 640x720 is the window. ImageMagick's import rejects
# decimal window ids, which is why nothing here says -window $wid.
shot() {
    import -window root -crop 640x720+0+0 +repage "$OUT/$1.png" \
        && echo "captured $1" || echo "CAPTURE FAILED: $1" >&2
}

# Dialogs are separate toplevels outside the app window, so they need
# the whole root, not the crop.
rootshot() {
    import -window root "$OUT/$1.png" \
        && echo "captured $1 (root)" || echo "CAPTURE FAILED: $1" >&2
}

# Wait for the app window, then force the geometry. Prints the window
# id, or returns 1.
find_window() {
    local wid="" attempt
    for attempt in $(seq 1 "$WINDOW_WAIT_ATTEMPTS"); do
        sleep "$WINDOW_WAIT_SECONDS"
        wid=$("$XDTOOL" search --name Aurora 2>/dev/null | head -1)
        [ -n "$wid" ] && break
    done
    if [ -z "$wid" ]; then
        return 1
    fi
    "$XDTOOL" windowsize "$wid" 640 720
    "$XDTOOL" windowmove "$wid" 0 0
    sleep 1
    echo "$wid"
}

# Close whatever dialog is on top by focusing it and sending Escape.
# Without a window manager nothing is focused by default, so the key
# has to be aimed.
dismiss_dialog() {
    local app_wid="$1" dialog_wid
    dialog_wid=$("$XDTOOL" search --class aurora-gui 2>/dev/null \
        | grep -v "^$app_wid$" | tail -1)
    if [ -n "$dialog_wid" ]; then
        "$XDTOOL" windowfocus "$dialog_wid"
        sleep 0.3
        "$XDTOOL" key Escape
        sleep 0.5
        "$XDTOOL" windowfocus "$app_wid"
    fi
}

# --- connected pass ----------------------------------------------------

capture_connected() {
    if ! systemctl --user is-active aurora.service >/dev/null 2>&1; then
        echo "daemon not running; start it first:  systemctl --user start aurora" >&2
        return 1
    fi

    export HOME="$HOME_DIR"
    export XDG_CONFIG_HOME="$HOME_DIR/.config"
    export XDG_CACHE_HOME="$HOME_DIR/.cache"
    export XDG_RUNTIME_DIR="/run/user/$(id -u)"
    export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
    export GDK_BACKEND=x11
    export GSK_RENDERER=cairo

    local prev_slot
    prev_slot=$(aurora slot | head -1)
    echo "slot before run: $prev_slot"

    aurora-gui >"$WORK/gui.log" 2>&1 &
    local app_pid=$!

    local wid
    wid=$(find_window) || {
        echo "window never appeared; see $WORK/gui.log" >&2
        kill "$app_pid" 2>/dev/null
        return 1
    }

    # Pages in tab order. Each check samples the tab pill, which only
    # exists under the selected tab, so a wrong page fails the check.
    shot 01-lighting
    check "01 selected tab" 01-lighting.png "$TAB1_X" "$TAB_Y" "56,56,59"
    check "01 keyboard deck" 01-lighting.png 320 200 "19,19,23"

    click "$TAB2_X" "$TAB_Y"
    shot 02-profiles
    check "02 selected tab" 02-profiles.png "$TAB2_X" "$TAB_Y" "56,56,59"

    click "$TAB3_X" "$TAB_Y"
    shot 03-custom
    check "03 selected tab" 03-custom.png "$TAB3_X" "$TAB_Y" "56,56,59"

    click "$TAB4_X" "$TAB_Y"
    shot 04-settings
    check "04 selected tab" 04-settings.png "$TAB4_X" "$TAB_Y" "56,56,59"
    # The battery switch reads the live GNOME accent, so the exact triple
    # follows gsettings; 51,128,221 is this machine's default blue.
    check "04 battery accent" 04-settings.png 538 260 "51,128,221"

    # Menu popover, then its second item: About Aurora.
    click "$MENU_X" "$TAB_Y"
    sleep 0.5
    shot 06-menu
    check "06 popover surface" 06-menu.png "$MENU_X" 60 "54,54,58"

    click "$MENU_X" "$ABOUT_ITEM_Y"
    sleep 1
    rootshot 07-about
    check_not "07 dialog present" 07-about.png 300 300 "34,34,38"
    dismiss_dialog "$wid"

    # Save Profile dialog: the + button on the Profiles group header.
    click "$TAB2_X" "$TAB_Y"
    sleep 0.5
    click "$PLUS_X" "$PLUS_Y"
    sleep 1
    rootshot 08-save-profile
    check_not "08 dialog present" 08-save-profile.png 100 150 "34,34,38"
    dismiss_dialog "$wid"

    # Backlight off, then put the slot back via the CLI rather than a
    # click, so a stale coordinate cannot change what the user had.
    click "$TAB1_X" "$TAB_Y"
    sleep 0.5
    click "$SLOT_OFF_X" "$SLOT_Y"
    sleep 0.6
    shot 05-lighting-off
    check "05 preview gone" 05-lighting-off.png 320 200 "34,34,38"
    if [ -n "$prev_slot" ]; then
        aurora slot "$prev_slot" >/dev/null
    fi
    echo "slot after run: $(aurora slot | head -1)"

    kill "$app_pid" 2>/dev/null
    wait "$app_pid" 2>/dev/null
}

# --- connecting pass ----------------------------------------------------

# A private XDG_RUNTIME_DIR hides the daemon socket (the app looks for
# it there), while the real session bus keeps startup working.
capture_connecting() {
    export HOME="$HOME_DIR"
    export XDG_CONFIG_HOME="$HOME_DIR/.config"
    export XDG_CACHE_HOME="$HOME_DIR/.cache"
    export XDG_RUNTIME_DIR="$WORK/runtime"
    export DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$(id -u)/bus"
    export GDK_BACKEND=x11
    export GSK_RENDERER=cairo

    aurora-gui >"$WORK/gui-connecting.log" 2>&1 &
    local app_pid=$!

    local wid
    wid=$(find_window) || {
        echo "window never appeared; see $WORK/gui-connecting.log" >&2
        kill "$app_pid" 2>/dev/null
        return 1
    }

    sleep 4
    shot 09-connecting
    check "09 status page" 09-connecting.png 320 400 "34,34,38"

    kill "$app_pid" 2>/dev/null
    wait "$app_pid" 2>/dev/null
}

# --- run ----------------------------------------------------------------

only="${1:-all}"

Xvfb ":$DISPLAY_NUM" -screen 0 900x800x24 -nolisten tcp &
xvfb_pid=$!
trap 'kill "$xvfb_pid" 2>/dev/null' EXIT
sleep 1

case "$only" in
    connected)  capture_connected ;;
    connecting) capture_connecting ;;
    all)        capture_connected; capture_connecting ;;
    *)          echo "usage: $0 [connected|connecting|all]" >&2; exit 1 ;;
esac

echo
echo "captures in $OUT:"
ls -l "$OUT" 2>/dev/null | awk 'NR > 1 { print "  " $9 " (" $5 " bytes)" }'

if [ -n "$failures" ]; then
    echo
    echo "checks that did not match (the UI has moved; re-measure with rowruns.sh):"
    echo "$failures"
    exit 1
fi
echo
echo "all pixel checks matched"
