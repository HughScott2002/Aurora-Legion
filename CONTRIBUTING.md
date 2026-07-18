# Contributing

Thanks for looking at Aurora. Discussions, issues and PRs are all
welcome; new frontends especially. The [`protocol`](protocol/) crate is
the seam (JSON lines over a unix socket), so a TUI, KDE or web client
needs zero daemon changes.

## Before you write code

Read [docs/style-guide.md](docs/style-guide.md). The short version: no
clever one-liners, bounded everything, no unwraps on daemon paths, stock
libadwaita in the GUI.

## Workflow

- Clone-to-running-build steps, devshell flags and the udev rule are in
  [docs/quick-start.md](docs/quick-start.md).
- Enable the local build gate once per clone:
  `git config core.hooksPath hooks`. It runs `nix build` before every
  push; there is no CI, the gate is the check.
- Verify changes against the real daemon, not just the compiler: run it,
  drive it with `aurora status` / `aurora set` or the GUI.

## Commits and PRs

- Conventional commits: `type(scope): imperative summary`, subject at
  most 72 characters, no trailing period.
- No AI attribution trailers.
- Body only for the non-obvious why, breaking changes or migrations.
- No em dashes in docs or user-facing strings.
- Performance claims need numbers: use `docs/measure.sh` and update
  `docs/measurements.md` alongside the README table.
