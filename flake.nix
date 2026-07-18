{
  description = "Daemon + GTK4 app to control the RGB keyboard of Lenovo Legion laptops";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane.url = "github:ipetkov/crane";

    flake-parts.url = "github:hercules-ci/flake-parts";

    systems = {
      url = "github:nix-systems/default-linux";
      flake = false;
    };

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs@{
      nixpkgs,
      flake-parts,
      systems,
      crane,
      rust-overlay,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = import systems;

      flake = {
        # Per-user service: installs the package and runs `legion-kb daemon`
        # as a systemd user service bound to the graphical session.
        homeModules.default =
          {
            config,
            lib,
            pkgs,
            ...
          }:
          let
            cfg = config.services.legion-kb-rgb;
          in
          {
            options.services.legion-kb-rgb = {
              enable = lib.mkEnableOption "the Legion keyboard RGB daemon";

              package = lib.mkOption {
                type = lib.types.package;
                default = inputs.self.packages.${pkgs.stdenv.hostPlatform.system}.default;
                description = "The legion-kb package to run.";
              };
            };

            config = lib.mkIf cfg.enable {
              home.packages = [ cfg.package ];

              systemd.user.services.legion-kb = {
                Unit = {
                  Description = "Legion keyboard RGB daemon";
                  # The ambient, ripple and hotkey features need the session
                  # environment (WAYLAND_DISPLAY/DISPLAY), hence
                  # graphical-session.target instead of default.target.
                  After = [ "graphical-session.target" ];
                  PartOf = [ "graphical-session.target" ];
                };
                Service = {
                  ExecStart = "${cfg.package}/bin/legion-kb daemon";
                  Restart = "on-failure";
                  RestartSec = 2;
                };
                Install = {
                  WantedBy = [ "graphical-session.target" ];
                };
              };
            };
          };

        # System-level piece: the udev rule that lets the logged-in user
        # open the keyboard's hidraw device without root.
        nixosModules.default =
          { config, lib, ... }:
          let
            cfg = config.hardware.legion-kb-rgb;

            # (vendor, product) pairs of every supported keyboard controller,
            # mirroring KNOWN_DEVICE_INFOS in driver/src/lib.rs.
            productIds = [
              "c955" # 2020
              "c963" # 2021 Ideapad
              "c965" # 2021
              "c973" # 2022 Ideapad
              "c975" # 2022
              "c983" # 2023 LOQ
              "c984" # 2023
              "c985" # 2023 Pro
              "c993" # 2024 LOQ
              "c994" # 2024
              "c995" # 2024 Pro
            ];

            ruleForProduct = productId: ''
              KERNEL=="hidraw*", SUBSYSTEMS=="usb", ATTRS{idVendor}=="048d", ATTRS{idProduct}=="${productId}", TAG+="uaccess"
            '';
          in
          {
            options.hardware.legion-kb-rgb = {
              enable = lib.mkEnableOption "udev rules granting seat users access to the Legion RGB keyboard";
            };

            config = lib.mkIf cfg.enable {
              services.udev.extraRules = lib.concatStrings (map ruleForProduct productIds);
            };
          };
      };

      perSystem =
        {
          pkgs,
          system,
          lib,
          ...
        }:
        let
          rustVersion = "1.94.0";

          rust = pkgs.rust-bin.stable.${rustVersion}.default.override {
            extensions = [
              "rust-src" # rust-analyzer
            ];
          };

          craneLib = (crane.mkLib pkgs).overrideToolchain rust;

          # Libraries needed both at compile and runtime
          sharedDeps = with pkgs; [
            dbus
            libx11
            fontconfig
            udev
            glib
            gst_all_1.gstreamer
            gst_all_1.gst-plugins-base
            libxi
            libusb1
            expat
            openssl

            # GTK4 GUI
            pango
            gdk-pixbuf
            gtk4
            libadwaita
          ];

          # Libraries needed at runtime
          runtimeDeps =
            with pkgs;
            [
              libxcursor
              libxcb
              freetype
              libxrandr
              wayland
              libxkbcommon
            ]
            ++ sharedDeps;

          buildEnvVars = {
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
          };

          # Allow a few more files to be included in the build workspace
          workspaceSrc = ./.;
          workspaceSrcString = builtins.toString workspaceSrc;

          dataFileFilter =
            path: _type: (lib.hasPrefix "${workspaceSrcString}/gui/data/" path) || (lib.hasPrefix "${workspaceSrcString}/systemd/" path);
          workspaceFilter = path: type: (dataFileFilter path type) || (craneLib.filterCargoSources path type);

          src = lib.cleanSourceWith {
            src = workspaceSrc;
            filter = workspaceFilter;
          };

          # https://github.com/NixOS/nixpkgs/blob/nixos-unstable/pkgs/by-name/ru/rustdesk/package.nix
          buildInputs =
            with pkgs;
            [
              libvpx
              libyuv
              libaom
            ]
            ++ sharedDeps;

          nativeBuildInputs = with pkgs; [
            pkg-config
            cmake
            clang
            wrapGAppsHook4
          ];

          # Forgo using VCPKG hacks on local builds because pain
          cargoExtraArgs = ''--locked --features "scrap/linux-pkg-config"'';

          stdenv = p: (p.stdenvAdapters.useMoldLinker p.stdenv);

          inherit (craneLib.crateNameFromCargoToml { cargoToml = ./daemon/Cargo.toml; }) pname version;

          # Vendor dependencies to fix webm-sys compile issue
          # https://github.com/NixOS/nixpkgs/pull/475893
          # https://crane.dev/patching_dependency_sources.html
          cargoVendorDir = craneLib.vendorCargoDeps {
            inherit src;

            overrideVendorGitCheckout =
              ps: drv:
              let
                hasPackageNamed = name: lib.any (p: p.name == name) ps;
                isRustWebmRepo = lib.any (
                  p: lib.hasPrefix "git+https://github.com/rustdesk-org/rust-webm" p.source
                ) ps;
              in
              # Technically both webm and webm-sys come from the same repo/"set"
              if isRustWebmRepo && (hasPackageNamed "webm-sys" || hasPackageNamed "webm") then
                drv.overrideAttrs (old: {
                  postPatch = (old.postPatch or "") + ''
                    sed -e '1i #include <cstdint>' -i "src/sys/libwebm/mkvparser/mkvparser.cc"
                  '';
                })
              else
                drv;
          };

          commonArgs = {
            inherit
              pname
              version
              src
              buildInputs
              nativeBuildInputs
              cargoExtraArgs
              stdenv
              cargoVendorDir
              ;
          }
          // buildEnvVars;

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          # Daemon + CLI (`legion-kb`) and GTK4 GUI (`legion-kb-gui`)
          legion-kb = craneLib.buildPackage (
            commonArgs
            // {
              meta.mainProgram = pname;
              inherit cargoArtifacts;

              doCheck = true;

              postInstall = ''
                install -Dm444 gui/data/com.github.hugh.LegionKbRgb.desktop -t $out/share/applications
                install -Dm444 gui/data/icons/hicolor/scalable/apps/com.github.hugh.LegionKbRgb.svg -t $out/share/icons/hicolor/scalable/apps

                install -Dm444 systemd/legion-kb.service -t $out/lib/systemd/user
                substituteInPlace $out/lib/systemd/user/legion-kb.service \
                  --replace-fail "ExecStart=legion-kb daemon" "ExecStart=$out/bin/legion-kb daemon"
              '';

              # wrapGAppsHook4 turns bin/* into wrapper scripts; patch the
              # actual ELF files wherever they ended up.
              postFixup = ''
                for exe in $out/bin/* $out/bin/.*-wrapped; do
                  if [ -f "$exe" ] && isELF "$exe"; then
                    patchelf --add-rpath "${lib.makeLibraryPath runtimeDeps}" "$exe"
                  fi
                done
              '';
            }
          );
        in
        {
          _module.args.pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };

          packages.default = legion-kb;

          apps.default.program = "${legion-kb}/bin/legion-kb-gui";
          apps.daemon.program = "${legion-kb}/bin/${pname}";

          devShells.default =
            let
              deps = buildInputs ++ nativeBuildInputs ++ runtimeDeps;
            in
            pkgs.mkShell {
              LD_LIBRARY_PATH = lib.makeLibraryPath deps;
              RUST_BACKTRACE = "1";
              inherit (buildEnvVars) LIBCLANG_PATH;

              buildInputs = [ rust ] ++ deps;
            };
        };
    };
}
