{
  description =
    "True Pixel: a shell with every capture dependency, nothing to install";

  # Pinned to the same nixpkgs revision as the project (see the root
  # flake.lock), so the tools the captures run with cannot drift from
  # what the app itself was built against. Bump this rev in lockstep
  # with the root lock.
  inputs.nixpkgs.url =
    "github:NixOS/nixpkgs/e607cb5360ff1234862ac9f8839522becb853bb9";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      # Everything capture.sh calls by name, except the app itself and
      # systemctl. The app comes from the profile on purpose: the
      # harness captures what is installed and running, not what the
      # working tree says. Xvfb ships in the xorg-server package;
      # fc-cache in fontconfig.
      captureTools = pkgs: [
        pkgs.xdotool
        pkgs.imagemagick
        pkgs.xorg-server
        pkgs.fontconfig
      ];

      shellFor =
        pkgs:
        pkgs.mkShellNoCC {
          packages = captureTools pkgs;
        };

      shellOutput =
        system:
        {
          name = system;
          value = {
            default = shellFor nixpkgs.legacyPackages.${system};
          };
        };
    in
    {
      devShells = builtins.listToAttrs (map shellOutput systems);
    };
}
