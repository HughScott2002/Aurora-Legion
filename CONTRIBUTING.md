# Contributing

Issues and pull requests are welcome. New frontends use the
[`protocol`](protocol/) crate and its
[documented interface](docs/protocol.md), so they need no daemon
changes.

## Before you write code

Read the [style guide](docs/style-guide.md). The short version: no
clever one-liners, bound everything, no `unwrap` or `expect` on daemon
paths, and use stock libadwaita in the GUI.

## Workflow

- Follow [Build from source](docs/how-to/build-from-source.md) for the
  development shell, native dependencies and checks.
- Enable the local gate once per clone: `git config core.hooksPath hooks`.
  Entering the development shell does it for you. There is no CI beyond
  the release workflow, so the gate is the check. At commit time it
  rejects AI attribution trailers and unformatted Rust, which costs
  about a tenth of a second, and nothing at all for a commit that
  touches no Rust. At push time it rescans the outgoing commits for
  trailers, then checks formatting and runs clippy, the tests and a
  cargo build. Those run inside `nix develop` when nix is present and
  call cargo directly when it is not, so nix is not required to
  contribute. Skip the compile stages with `SKIP_BUILD=1 git push`; the
  trailer scan always runs.
- Point git at the reformat list once, so `git blame` looks past the
  commits that only moved whitespace:
  `git config blame.ignoreRevsFile .git-blame-ignore-revs`.
- Verify changes against the real daemon, not just the compiler: run it,
  drive it with `aurora status` / `aurora set` or the GUI.
- Put user documentation in the right
  [Diátaxis section](docs/README.md) and keep one purpose per page.

## Commits and PRs

- Conventional commits: `type(scope): imperative summary`, subject at
  most 72 characters, no trailing period.
- Sign your commits. Set it up once per machine:

  ```console
  $ git config --global gpg.format ssh
  $ git config --global user.signingkey ~/.ssh/id_ed25519.pub
  $ git config --global commit.gpgsign true
  ```

  Add that same public key to your GitHub account a second time, as a
  signing key rather than an authentication key, or GitHub shows the
  commits as unverified even though they are signed. GPG works just as
  well if you already use it: skip the `gpg.format` line and set
  `user.signingkey` to your key ID. Check your own work with
  `git log --format='%h %G? %s'`, where `G` is a good signature.
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
6. Commit as `chore(release): vX.Y.Z` and push (the pre-push gate runs
   clippy, the tests and a build). Run `nix build` yourself as well:
   it is what the release workflow packages and it is not a gate stage.
7. Tag and push the tag: `git tag vX.Y.Z && git push origin vX.Y.Z`.
8. Watch the workflow (`gh run watch`), then download the published
   asset and confirm it unpacks and `bin/aurora --help` runs.
