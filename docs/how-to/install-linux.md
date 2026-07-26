# Install Aurora on other Linux distributions

Use the AppImage for the shortest install. Use the tarball when you
want a native binary and a user service.

Both builds require x86_64 and glibc 2.39 or newer:

```console
$ uname -m
x86_64
$ ldd --version
```

## Grant keyboard access

Every install needs Aurora's udev rule:

```console
$ curl -fsSLo /tmp/99-aurora.rules \
    https://raw.githubusercontent.com/HughScott2002/Aurora-Legion/main/udev/99-aurora.rules
$ sudo install -Dm644 /tmp/99-aurora.rules \
    /etc/udev/rules.d/99-aurora.rules
$ sudo udevadm control --reload-rules
$ sudo udevadm trigger
```

Replug the keyboard or reboot.

## Install the AppImage

Download `Aurora-<version>-x86_64.AppImage` from the
[latest release](https://github.com/HughScott2002/Aurora-Legion/releases).
Rename it to `Aurora.AppImage`, then install it:

```console
$ install -Dm755 Aurora.AppImage "$HOME/.local/bin/Aurora.AppImage"
$ "$HOME/.local/bin/Aurora.AppImage"
```

With no arguments, the AppImage starts the daemon if needed and opens
the GUI. Pass CLI commands as arguments:

```console
$ "$HOME/.local/bin/Aurora.AppImage" status
```

## Install the tarball

The tarball also needs GTK 4.14 or newer. Download
`aurora-<version>-x86_64-linux-gnu.tar.gz` from the
[latest release](https://github.com/HughScott2002/Aurora-Legion/releases)
and rename it to `aurora.tar.gz`. Then unpack it:

```console
$ mkdir aurora-install
$ tar -xzf aurora.tar.gz -C aurora-install --strip-components=1
$ cd aurora-install
$ ./install.sh
```

The installer stays under `~/.local` except for the udev rule, which it
asks before installing. Its `README.txt` lists the required runtime
packages and every installed file.

## Verify

```console
$ aurora status
daemon:   running (v0.22.0)
keyboard: connected
```

For the AppImage, replace `aurora` with its full path.

If your distribution is too old for either build, follow
[Build from source](build-from-source.md). For failures, use
[Troubleshoot Aurora](troubleshoot.md).
