# Install Aurora on NixOS

There are two ways in. Pick one; they are alternatives, not steps.

- **NixOS alone.** One option installs everything. Use this unless you
  already manage your user environment with Home Manager.
- **NixOS plus Home Manager.** The daemon is declared per user, and the
  NixOS side only grants keyboard access.

Both end with the same thing running: `aurora daemon` as a systemd user
service, started with your graphical session.

## Add the flake input

```nix
inputs.aurora.url = "github:HughScott2002/Aurora-Legion";
```

Pass `aurora` to the module arguments in your usual flake output.

## Option 1: NixOS alone

```nix
{
  imports = [ aurora.nixosModules.default ];

  services.aurora.enable = true;
}
```

That one option does three things: installs the package into
`environment.systemPackages`, installs the udev rules for every
supported controller, and declares the daemon as a systemd user service
bound to `graphical-session.target`, restarting on failure.

The service is declared for every user but only starts inside a
graphical session, and the udev `uaccess` rule scopes device access to
the seat user.

To run a different build, set `services.aurora.package`.

Apply it:

```console
$ sudo nixos-rebuild switch --flake .#HOSTNAME
```

## Option 2: NixOS plus Home Manager

Grant keyboard access at the system level:

```nix
{
  imports = [ aurora.nixosModules.default ];

  hardware.aurora.enable = true;
}
```

`hardware.aurora.enable` installs the udev rules and nothing else. Do
not set `services.aurora.enable` as well; it would declare a second
daemon service alongside the Home Manager one.

Then run the daemon for your user:

```nix
{
  imports = [ aurora.homeModules.default ];

  services.aurora.enable = true;
}
```

Apply both:

```console
$ sudo nixos-rebuild switch --flake .#HOSTNAME
$ home-manager switch --flake .#USERNAME@HOSTNAME
```

Replace the flake output names with yours.

## Verify

Either way:

```console
$ systemctl --user status aurora --no-pager
$ aurora status
```

The second command should report `keyboard: connected`.

If the service has not followed the graphical session yet, start it
once:

```console
$ systemctl --user start aurora
```

Continue with the [quick start](../quick-start.md#2-test-the-slots).
