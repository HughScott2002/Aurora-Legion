# Install Aurora with an AI assistant

This is the canonical procedure for an agent installing Aurora.

Aurora controls supported Lenovo 4-zone RGB keyboards through a
persistent daemon. The GUI and CLI are clients.

## Rules

- Ask before every command that uses `sudo`.
- Outside the user's home directory, create only
  `/etc/udev/rules.d/99-aurora.rules`.
- Do not stop unrelated services or remove another lighting tool
  without permission.
- Report failures. Use Aurora's troubleshooting guide instead of
  improvising system changes.

## Inspect the machine

Run read-only checks:

```console
$ cat /etc/os-release
$ uname -m
$ ldd --version
$ lsusb
```

Prebuilt releases require `x86_64`. A supported keyboard appears with
vendor ID `048d` and one of these product IDs:

```text
c955 c963 c965 c973 c975 c983 c984 c985 c993 c994 c995
```

If vendor `048d` has another product ID, stop and help the user open an
[unsupported keyboard issue](https://github.com/HughScott2002/Aurora-Legion/issues/new).
Include the `lsusb` line. If no matching device exists, stop.

## Choose one guide

Fetch and follow one canonical guide:

1. NixOS:
   <https://raw.githubusercontent.com/HughScott2002/Aurora-Legion/main/docs/how-to/install-nixos.md>
2. Other Linux with glibc 2.39 or newer:
   <https://raw.githubusercontent.com/HughScott2002/Aurora-Legion/main/docs/how-to/install-linux.md>
3. Older Linux or a requested source build:
   <https://raw.githubusercontent.com/HughScott2002/Aurora-Legion/main/docs/how-to/build-from-source.md>

Do not combine install methods. Complete the selected guide before
verification.

## Verify

Use the installed CLI or the AppImage path:

```console
$ aurora status
daemon:   running (v0.23.0)
keyboard: connected
$ aurora set -e Static \
    -c 255,0,0,0,255,0,0,0,255,255,255,255
profile applied
```

Ask the user whether the four zones changed to red, green, blue and
white. Open the GUI and confirm it connects.

If any check fails, follow:

<https://raw.githubusercontent.com/HughScott2002/Aurora-Legion/main/docs/how-to/troubleshoot.md>

## Report

Tell the user:

- which install method you used;
- where Aurora was installed;
- whether the daemon and keyboard connected;
- which privileged command they approved;
- how to uninstall the selected method.

For a manual AppImage install, uninstall its file and any user service
or desktop entry created for it. For the tarball, use its `README.txt`
file list. Remove the udev rule only if the user asks.
