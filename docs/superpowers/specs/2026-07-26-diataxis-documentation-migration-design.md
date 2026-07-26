# Diátaxis documentation migration design

Date: 2026-07-26
Status: approved

## Goal

Give Aurora a concise documentation system in which each page has one
job. A reader should reach the right page from the repository README in
two clicks or fewer.

## Readers

- A Linux user who wants working keyboard lighting.
- A contributor who wants to build and change Aurora.
- A frontend author who needs the IPC contract.
- A maintainer investigating the ITE controller.

## Structure

`README.md` remains the product front door. It explains what Aurora is,
shows the result and links to `docs/README.md`.

`docs/README.md` routes readers through the four Diátaxis modes:

| Mode | Purpose | Canonical pages |
| --- | --- | --- |
| Tutorial | Learn by reaching a visible result | `quick-start.md` |
| How-to | Complete a specific task | NixOS install, other Linux install, source build, troubleshooting, Fn+Space slots, AI-assisted install |
| Reference | Look up exact facts | CLI, IPC protocol, runtime files, measurements, code style |
| Explanation | Understand design and tradeoffs | Architecture, Fn+Space synchronization |

Research notes remain under `docs/research/`. They preserve evidence and
unknowns. Planning records remain under `docs/superpowers/`. Neither is
mixed into task guidance.

## Target pages

### Tutorial

- `docs/quick-start.md`: start Aurora, set a visible profile and confirm
  Fn+Space cycles red, green, blue and off.

### How-to guides

- `docs/how-to/install-nixos.md`
- `docs/how-to/install-linux.md`
- `docs/how-to/build-from-source.md`
- `docs/how-to/troubleshoot.md`
- `docs/how-to/use-fn-space-slots.md`
- `docs/install-with-ai.md`

### Reference

- `docs/reference/cli.md`
- `docs/protocol.md`
- `docs/reference/runtime-files.md`
- `docs/measurements.md`
- `docs/style-guide.md`

### Explanation

- `docs/explanation/architecture.md`
- `docs/explanation/fn-space-sync.md`

## Migration rules

Existing public paths stay valid. The migration rewrites
`quick-start.md`, `install-with-ai.md`, `protocol.md`,
`measurements.md` and `style-guide.md` in place. New pages receive
content extracted from the README or quick start.

Each fact has one canonical home. Other pages link to it instead of
copying it. In particular:

- Install commands live in install guides.
- Build and verification commands live in the source-build guide.
- Exact CLI syntax lives in CLI reference.
- Paths and limits live in reference pages.
- Reasons and tradeoffs live in explanation pages.
- Hardware evidence stays in the ITE research note.

The repository README keeps only the shortest working install path and
the links needed to choose another path.

The untracked
`docs/research/mature-daemon-native-gui-references.md` file is outside
this migration and must not be staged.

## Writing rule

Treat every reader as important. Cut every word that does not help that
reader act or understand.

Use direct verbs, short paragraphs and concrete nouns. Avoid throat
clearing, repeated summaries, sales language and needless warnings.
Keep one purpose per page. Do not use em dashes or en dashes.

## Architecture language

Documentation must match Aurora's current architecture:

- The daemon core module alone mutates daemon state.
- Other daemon modules send bounded commands to the core interface.
- The protocol crate is the UI-free interface at the client seam.
- The GUI and CLI are adapters at that seam.
- The driver module owns controller-specific HID implementation.

The documentation branch will not refactor product code. It will expose
architectural friction only when the existing interface makes an
accurate explanation impossible.

## Verification

- Check commands against the built CLI and repository configuration.
- Check every relative Markdown link.
- Check generated English prose for forbidden punctuation.
- Run `git diff --check`.
- Run `nix build`.
- Confirm the untracked research file remains untracked.
- Confirm `dev` contains the feature merge and the documentation branch
  starts from `dev`.

## Non-goals

- Product code changes.
- GUI redesign.
- Rewriting source-backed research into unsourced claims.
- Publishing or pushing branches.
