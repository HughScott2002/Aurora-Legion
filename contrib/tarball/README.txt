Aurora prebuilt tarball (x86_64-linux-gnu)
==========================================

Aurora controls the 4-zone RGB keyboard of select 2020 to 2024 Lenovo
Legion, IdeaPad, and LOQ laptops. Project page and docs:

  https://github.com/HughScott2002/Aurora-Legion

Contents
--------

  bin/aurora            daemon + CLI
  bin/aurora-gui        GTK4/libadwaita app
  share/                desktop entry and icon
  systemd/              user service unit
  udev/99-aurora.rules  keyboard access rules
  install.sh            user-level installer (see below)

Runtime requirements
--------------------

Built on Ubuntu 24.04 LTS; runs there and on anything newer.

  glibc >= 2.39
  GTK >= 4.14 and libadwaita >= 1.5
  gstreamer 1.0 with base plugins
  libvpx, libaom, libyuv
  libusb 1.0

Ubuntu 24.04 / Debian 13 (verified):

  sudo apt install libgtk-4-1 libadwaita-1-0 libgstreamer1.0-0 \
    gstreamer1.0-plugins-base libvpx9 libaom3 libyuv0 libusb-1.0-0

Fedora 40+ (unverified, names may drift):

  sudo dnf install gtk4 libadwaita gstreamer1 gstreamer1-plugins-base \
    libvpx aom libyuv libusb1

Arch (unverified, names may drift):

  sudo pacman -S gtk4 libadwaita gstreamer gst-plugins-base \
    libvpx aom libyuv libusb

Install
-------

  ./install.sh

Installs to ~/.local (binaries, desktop entry, icon) and
~/.config/systemd/user (service), then offers to install the udev rule
to /etc/udev/rules.d (the only sudo step). Replug the keyboard after
the udev step, then check:

  ~/.local/bin/aurora status

Uninstall
---------

  systemctl --user disable --now aurora
  rm ~/.local/bin/aurora ~/.local/bin/aurora-gui
  rm ~/.local/share/applications/io.github.HughScott2002.Aurora.desktop
  rm ~/.local/share/icons/hicolor/scalable/apps/io.github.HughScott2002.Aurora.svg
  rm ~/.config/systemd/user/aurora.service
  sudo rm /etc/udev/rules.d/99-aurora.rules
