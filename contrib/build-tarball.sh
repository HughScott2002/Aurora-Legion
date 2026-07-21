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

# Keep in sync with rustVersion in flake.nix.
RUST_VERSION="1.94.0"

# The apt list below is the verified build list; docs/quick-start.md
# quotes it. If you change it here, change it there.
BUILD_DEPS="
  build-essential
  pkg-config
  cmake
  clang
  libclang-dev
  git
  curl
  ca-certificates
  libgtk-4-dev
  libadwaita-1-dev
  libgstreamer1.0-dev
  libgstreamer-plugins-base1.0-dev
  libvpx-dev
  libaom-dev
  libyuv-dev
  libusb-1.0-0-dev
  libudev-dev
  libssl-dev
  libx11-dev
  libxi-dev
  libxtst-dev
  libxcb1-dev
  libxcb-shm0-dev
  libxcb-randr0-dev
  libdbus-1-dev
"

require_ubuntu_2404() {
  if [ "${AURORA_TARBALL_ALLOW_HOST:-0}" = "1" ]; then
    echo "warning: host check skipped (AURORA_TARBALL_ALLOW_HOST=1)" >&2
    return
  fi
  if ! grep -q 'VERSION_ID="24.04"' /etc/os-release; then
    echo "error: this script must run on Ubuntu 24.04 so the produced" >&2
    echo "binaries match the supported glibc/GTK baseline." >&2
    echo "Run it in docker (see the header comment), or set" >&2
    echo "AURORA_TARBALL_ALLOW_HOST=1 to override." >&2
    exit 1
  fi
}

install_build_deps() {
  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  # shellcheck disable=SC2086
  apt-get install -y --no-install-recommends $BUILD_DEPS
}

install_rust() {
  if ! command -v cargo >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain "$RUST_VERSION" --profile minimal
  fi
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
  rustup toolchain install "$RUST_VERSION" --profile minimal
  rustup default "$RUST_VERSION"
}

write_libyuv_pc() {
  # Ubuntu's libyuv-dev ships headers and libraries but no pkg-config
  # file, and scrap's linux-pkg-config feature needs one. Point at the
  # packaged locations.
  mkdir -p /build/pkgconfig
  cat >/build/pkgconfig/libyuv.pc <<'EOF'
prefix=/usr
libdir=/usr/lib/x86_64-linux-gnu
includedir=/usr/include

Name: libyuv
Description: YUV scaling and conversion library
Version: 0
Libs: -L${libdir} -lyuv
Cflags: -I${includedir}
EOF
  export PKG_CONFIG_PATH="/build/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
}

build_workspace() {
  # webm-sys fails to compile without cstdint being force-included.
  export CXXFLAGS="-include cstdint"
  # Keep build output off the (possibly bind-mounted) source tree so the
  # host does not end up with a root-owned target directory.
  export CARGO_TARGET_DIR="${AURORA_TARGET_DIR:-/build/target}"
  cargo build --release --workspace --locked --features aurora/scrap-pkg-config
}

read_version() {
  local line
  line=$(grep -m1 '^version' daemon/Cargo.toml)
  VERSION=$(echo "$line" | cut -d '"' -f 2)
  if [ -z "$VERSION" ]; then
    echo "error: could not read version from daemon/Cargo.toml" >&2
    exit 1
  fi
}

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

restore_dist_ownership() {
  # When run via docker on a bind mount, dist/ would otherwise be owned
  # by root on the host.
  local owner
  owner=$(stat -c '%u:%g' .)
  if [ "$owner" != "$(id -u):$(id -g)" ]; then
    chown -R "$owner" dist
  fi
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
