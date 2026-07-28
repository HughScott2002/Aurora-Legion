# Documentation

Choose the page that matches your job.

## Start here

[Quick start](quick-start.md) takes you from launch to a visible profile
and a working Fn+Space cycle.

## Tutorials

- [Quick start](quick-start.md): learn Aurora by running it and changing
  the keyboard.

## How-to guides

- [Install on NixOS](how-to/install-nixos.md): install Aurora, enable
  keyboard access and start the user service.
- [Install on other Linux](how-to/install-linux.md): use the AppImage or
  prebuilt tarball.
- [Build from source](how-to/build-from-source.md): prepare the toolchain,
  build Aurora and run its checks.
- [Troubleshoot Aurora](how-to/troubleshoot.md): diagnose daemon,
  permission, GUI and Fn+Space failures.
- [Use Fn+Space slots](how-to/use-fn-space-slots.md): select and edit
  the three lighting slots every profile holds.
- [Install with an AI assistant](install-with-ai.md): give an agent a
  bounded, auditable installation procedure.

## Reference

- [CLI](reference/cli.md): commands, options, effects and examples.
- [IPC protocol](protocol.md): the complete JSON-lines client contract.
- [Runtime files](reference/runtime-files.md): sockets, settings, units,
  rules and logs.
- [Measurements](measurements.md): method and raw performance results.
- [Code style](style-guide.md): Rust, daemon and GTK rules.
- [Interface style](ui-style-guide.md): hierarchy, spacing and what
  earns a place on screen.
- [Contributing](../CONTRIBUTING.md): commits, checks and releases.

## Explanation

- [Architecture](explanation/architecture.md): why Aurora separates a
  persistent daemon from transient clients.
- [Fn+Space synchronization](explanation/fn-space-sync.md): why Aurora
  counts WMI events instead of polling the EC profile counter.
- [Roadmap](../ROADMAP.md): the order and limits of planned platform
  work.

## Research

- [ITE 8295 hardware profiles](research/ite8295-hardware-profiles.md):
  sources, experiments and remaining unknowns.
- [Mature daemon and native GUI references](research/mature-daemon-native-gui-references.md):
  the projects Aurora's architecture was checked against, and what not
  to copy from them.
