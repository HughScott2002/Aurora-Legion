#!/usr/bin/env bash
# Build the Aurora AppImage inside an Ubuntu 24.04 environment.
#
# The AppImage bundles both binaries plus GTK4, libadwaita and the rest
# of the linked libraries, so it runs on most x86_64 distros without
# installing packages. Run it from the repo root via docker:
#
#   docker run --rm -v "$PWD:/src" -w /src ubuntu:24.04 \
#     bash contrib/build-appimage.sh
#
# The release workflow runs build-tarball.sh first in the same job, so
# the cargo build here is a cache hit. Output lands in dist/.

set -eu

# shellcheck source=contrib/build-common.sh
. "$(dirname "$0")/build-common.sh"

# Tools that only the AppImage build needs.
APPIMAGE_DEPS="
  wget
  file
  desktop-file-utils
  librsvg2-common
"

# linuxdeploy walks the binaries' dependencies into AppDir/usr/lib and
# appimagetool packs the result. Both only publish rolling builds under
# the continuous tag.
LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
APPIMAGETOOL_URL="https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"

install_appimage_deps() {
  export DEBIAN_FRONTEND=noninteractive
  # shellcheck disable=SC2086
  apt-get install -y --no-install-recommends $APPIMAGE_DEPS
}

fetch_tools() {
  mkdir -p /build/tools
  if [ ! -x /build/tools/linuxdeploy ]; then
    wget -q -O /build/tools/linuxdeploy "$LINUXDEPLOY_URL"
    chmod +x /build/tools/linuxdeploy
  fi
  if [ ! -x /build/tools/appimagetool ]; then
    wget -q -O /build/tools/appimagetool "$APPIMAGETOOL_URL"
    chmod +x /build/tools/appimagetool
  fi
}

make_appdir() {
  APPDIR="/build/AppDir"
  rm -rf "$APPDIR"
  mkdir -p "$APPDIR"

  install -Dm755 "$CARGO_TARGET_DIR/release/aurora" "$APPDIR/usr/bin/aurora"
  install -Dm755 "$CARGO_TARGET_DIR/release/aurora-gui" "$APPDIR/usr/bin/aurora-gui"
  strip "$APPDIR/usr/bin/aurora" "$APPDIR/usr/bin/aurora-gui"

  # The container has no FUSE; the tools self-extract with this set.
  export APPIMAGE_EXTRACT_AND_RUN=1

  # libusb and libfribidi are on linuxdeploy's default exclude list
  # (assumed present on desktop systems) but minimal installs can lack
  # them; bundle them explicitly.
  /build/tools/linuxdeploy \
    --appdir "$APPDIR" \
    --executable "$APPDIR/usr/bin/aurora" \
    --executable "$APPDIR/usr/bin/aurora-gui" \
    --library /usr/lib/x86_64-linux-gnu/libusb-1.0.so.0 \
    --library /usr/lib/x86_64-linux-gnu/libfribidi.so.0 \
    --library /usr/lib/x86_64-linux-gnu/libwayland-client.so.0 \
    --library /usr/lib/x86_64-linux-gnu/libwayland-cursor.so.0 \
    --library /usr/lib/x86_64-linux-gnu/libwayland-egl.so.1 \
    --desktop-file gui/data/io.github.HughScott2002.Aurora.desktop \
    --icon-file gui/data/icons/hicolor/scalable/apps/io.github.HughScott2002.Aurora.svg \
    --custom-apprun contrib/appimage/AppRun

  # linuxdeploy bundles libraries; the GTK runtime data has to be added
  # by hand. Compiled gsettings schemas cover GTK4 and libadwaita on
  # hosts too old to have their own.
  install -Dm644 /usr/share/glib-2.0/schemas/gschemas.compiled \
    "$APPDIR/usr/share/glib-2.0/schemas/gschemas.compiled"
}

pack_appimage() {
  local name
  name="Aurora-${VERSION}-x86_64.AppImage"
  mkdir -p dist
  ARCH=x86_64 /build/tools/appimagetool "$APPDIR" "dist/$name"
  sha256sum "dist/$name"
}

require_ubuntu_2404
install_build_deps
install_appimage_deps
install_rust
write_libyuv_pc
build_workspace
read_version
fetch_tools
make_appdir
pack_appimage
restore_dist_ownership

echo "done: dist/Aurora-${VERSION}-x86_64.AppImage"
