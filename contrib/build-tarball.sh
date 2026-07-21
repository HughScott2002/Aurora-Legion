#!/usr/bin/env bash
# Build the prebuilt Aurora tarball inside an Ubuntu 24.04 environment.
#
# Ubuntu 24.04 LTS is the oldest supported baseline (glibc 2.39,
# GTK 4.14, libadwaita 1.5), so binaries built here run on it and on
# anything newer. Run it from the repo root via docker:
#
#   docker run --rm -v "$PWD:/src" -w /src ubuntu:24.04 \
#     bash contrib/build-tarball.sh
#
# The release workflow (.github/workflows/release.yml) runs the same
# script in the same container image. Output lands in dist/.

set -eu

# shellcheck source=contrib/build-common.sh
. "$(dirname "$0")/build-common.sh"

stage_and_pack() {
  local name stage
  name="aurora-${VERSION}-x86_64-linux-gnu"
  stage="/build/stage/$name"

  rm -rf "$stage"
  mkdir -p "$stage"

  install -Dm755 "$CARGO_TARGET_DIR/release/aurora" "$stage/bin/aurora"
  install -Dm755 "$CARGO_TARGET_DIR/release/aurora-gui" "$stage/bin/aurora-gui"
  strip "$stage/bin/aurora" "$stage/bin/aurora-gui"

  install -Dm644 gui/data/io.github.HughScott2002.Aurora.desktop \
    "$stage/share/applications/io.github.HughScott2002.Aurora.desktop"
  install -Dm644 gui/data/icons/hicolor/scalable/apps/io.github.HughScott2002.Aurora.svg \
    "$stage/share/icons/hicolor/scalable/apps/io.github.HughScott2002.Aurora.svg"
  install -Dm644 systemd/aurora.service "$stage/systemd/aurora.service"
  install -Dm644 udev/99-aurora.rules "$stage/udev/99-aurora.rules"
  install -Dm755 contrib/tarball/install.sh "$stage/install.sh"
  install -Dm644 contrib/tarball/README.txt "$stage/README.txt"

  mkdir -p dist
  tar -C "/build/stage" -czf "dist/$name.tar.gz" "$name"
  sha256sum "dist/$name.tar.gz"
}

require_ubuntu_2404
install_build_deps
install_rust
write_libyuv_pc
build_workspace
read_version
stage_and_pack
restore_dist_ownership

echo "done: dist/aurora-${VERSION}-x86_64-linux-gnu.tar.gz"
