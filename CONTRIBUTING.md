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

## Releases

Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds
the prebuilt tarball and the AppImage in an Ubuntu 24.04 container (the
same `contrib/build-tarball.sh` and `contrib/build-appimage.sh` you can
run locally in docker) and publishes a GitHub Release with the matching
`CHANGELOG.md` section as its notes.

To cut version X.Y.Z:

1. Bump `version` in `daemon/Cargo.toml` and `gui/Cargo.toml`, then run
   `cargo check` so `Cargo.lock` picks up the new versions.
2. Update the version fixture strings in `protocol/src/ipc.rs` tests so
   the examples stay honest.
3. Move the Unreleased entries in `CHANGELOG.md` under a new
   `## [X.Y.Z] - date` heading and update the link references. The
   workflow refuses to release a version whose changelog section is
   missing or empty.
4. Once the AppStream metainfo file exists (branding, issue #2), add a
   matching `<release version="X.Y.Z">` entry to it; the workflow
   fails without one.
5. Optionally verify the tarball locally first:
   `docker run --rm -v "$PWD:/src" -w /src ubuntu:24.04 bash contrib/build-tarball.sh`
6. Commit as `chore(release): vX.Y.Z` and push (the pre-push hook runs
   `nix build`).
7. Tag and push the tag: `git tag vX.Y.Z && git push origin vX.Y.Z`.
8. Watch the workflow (`gh run watch`), then download the published
   asset and confirm it unpacks and `bin/aurora --help` runs.
