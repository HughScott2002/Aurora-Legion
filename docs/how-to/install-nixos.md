# Install Aurora on NixOS

Aurora provides one Home Manager module for the daemon and one NixOS
module for keyboard access. Use both.

## Add the flake input

```nix
inputs.aurora.url = "github:HughScott2002/Aurora-Legion";
```

Pass `aurora` to the module arguments in your usual flake output.

## Enable keyboard access

Import the NixOS module:

```nix
{
  imports = [ aurora.nixosModules.default ];

  hardware.aurora.enable = true;
}
```

This installs the udev rules for every supported controller.

## Enable the daemon

Import the Home Manager module:

```nix
{
  imports = [ aurora.homeModules.default ];

  services.aurora.enable = true;
}
```

This installs Aurora and starts `aurora daemon` with a systemd user
service at the graphical session.

Apply both configurations with your normal flake commands. A common
layout uses:

```console
$ sudo nixos-rebuild switch --flake .#HOSTNAME
$ home-manager switch --flake .#USERNAME@HOSTNAME
```

Replace the flake output names with yours.

## Verify

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
