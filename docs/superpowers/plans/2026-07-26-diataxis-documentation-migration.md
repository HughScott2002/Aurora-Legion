# Diátaxis Documentation Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Aurora a concise, complete documentation system organized by reader need.

**Architecture:** Keep established public paths stable. Use `docs/README.md` as the map, then give each maintained page one Diátaxis purpose. Extract mixed content from the repository README and quick start into canonical task, reference and explanation pages.

**Tech Stack:** Markdown, Git, Aurora CLI help, Nix.

## Global Constraints

- Treat every reader as important and cut every unnecessary word.
- Use direct verbs, short paragraphs and concrete nouns.
- Do not use em dashes or en dashes.
- Keep each fact in one canonical page and link to it elsewhere.
- Preserve `docs/quick-start.md`, `docs/install-with-ai.md`, `docs/protocol.md`, `docs/measurements.md` and `docs/style-guide.md`.
- Keep research evidence under `docs/research/`.
- Do not stage `docs/research/mature-daemon-native-gui-references.md`.
- Do not change product code or add dependencies.
- Keep every maintained page within two clicks of `README.md`.

---

### Task 1: Build the documentation front door

**Files:**

- Modify: `README.md`
- Create: `docs/README.md`

**Interfaces:**

- Consumes: Existing product claims, install paths and documentation files.
- Produces: The only top-level route into tutorials, how-to guides, reference and explanation.

- [ ] **Step 1: Rewrite the repository README**

Keep the banner, screenshot, short product case, measured results,
architecture diagram, community links and credits. Replace the full
installation and CLI sections with:

- one Nix command pair that reaches a running daemon and GUI;
- one link to `docs/quick-start.md`;
- one link to `docs/README.md`;
- one sentence pointing non-Nix users to the install guides.

Keep supported-hardware and measurement claims linked to their evidence.

- [ ] **Step 2: Create the documentation index**

Create `docs/README.md` with these headings:

```markdown
# Documentation
## Start here
## Tutorials
## How-to guides
## Reference
## Explanation
## Research and design records
```

Give each link one sentence that says what the reader will accomplish or
learn. Link every maintained document exactly once in its primary mode.

- [ ] **Step 3: Check navigation**

Run:

```bash
git diff --check
for file in README.md docs/README.md; do
  /var/lib/community-skills/waza/skills/write/scripts/check-punctuation.sh --lang en "$file"
done
```

Expected: both commands exit 0.

- [ ] **Step 4: Commit the front door**

```bash
git add README.md docs/README.md
git commit -m "docs: add Diataxis documentation front door"
```

### Task 2: Separate the tutorial and task guides

**Files:**

- Modify: `docs/quick-start.md`
- Modify: `docs/install-with-ai.md`
- Create: `docs/how-to/install-nixos.md`
- Create: `docs/how-to/install-linux.md`
- Create: `docs/how-to/build-from-source.md`
- Create: `docs/how-to/troubleshoot.md`
- Create: `docs/how-to/use-fn-space-slots.md`

**Interfaces:**

- Consumes: Current install commands, udev rules, build flags, service behavior and verified slot behavior.
- Produces: One learning path and five task-focused guides.

- [ ] **Step 1: Rewrite the quick start as a tutorial**

Teach one path with Nix:

1. Start the daemon.
2. Open the GUI.
3. Apply a visible profile.
4. Close the GUI and confirm the lighting persists.
5. Press Fn+Space and confirm red, green, blue and off.

Link out for installation, source builds and troubleshooting. Do not
include package matrices or runtime path tables.

- [ ] **Step 2: Write the NixOS install guide**

Document the flake input, Home Manager module and NixOS udev module.
End with `systemctl --user status aurora` and `aurora status`.

- [ ] **Step 3: Write the other Linux install guide**

Present AppImage first and tarball second. State the x86_64 and glibc
requirements. Include the udev rule and a short verification sequence.
Link source builders to `build-from-source.md`.

- [ ] **Step 4: Write the source-build guide**

Document:

- `nix develop`;
- `CXXFLAGS="-include cstdint"`;
- `cargo build --workspace --features aurora/scrap-pkg-config`;
- the verified Ubuntu 24.04 package list;
- the `libyuv.pc` workaround;
- manual binary, desktop entry and user-unit installation;
- `nix build` as the required gate.

- [ ] **Step 5: Write the troubleshooting guide**

Give symptom-first sections for:

- daemon not running;
- keyboard not found or permission denied;
- another process owning the HID device;
- GUI startup failure;
- Fn+Space showing firmware RGB or darkness;
- controller state surviving reboot.

Use exact diagnostic commands and link the EC reset warning to the
hardware research note.

- [ ] **Step 6: Write the Fn+Space task guide**

Explain the visible default sequence, how a GUI or CLI lighting change
is saved into the active lit slot, how off behaves, and how to verify
the active slot with `aurora status`. State the known GUI follow-up
issues without presenting unfinished controls as available.

- [ ] **Step 7: Tighten the AI install guide**

Keep its safety rules and environment detection. Replace copied install
procedures with absolute raw GitHub links to the canonical install and
troubleshooting guides. Keep the final verification and uninstall
report requirements.

- [ ] **Step 8: Check and commit the task guides**

Run:

```bash
git diff --check
for file in docs/quick-start.md docs/install-with-ai.md docs/how-to/*.md; do
  /var/lib/community-skills/waza/skills/write/scripts/check-punctuation.sh --lang en "$file"
done
```

Expected: both commands exit 0.

Then commit:

```bash
git add docs/quick-start.md docs/install-with-ai.md docs/how-to/install-nixos.md docs/how-to/install-linux.md docs/how-to/build-from-source.md docs/how-to/troubleshoot.md docs/how-to/use-fn-space-slots.md
git commit -m "docs: add task-focused user guides"
```

### Task 3: Add reference and explanation

**Files:**

- Create: `docs/reference/cli.md`
- Create: `docs/reference/runtime-files.md`
- Create: `docs/explanation/architecture.md`
- Create: `docs/explanation/fn-space-sync.md`
- Modify: `docs/protocol.md`
- Modify: `docs/measurements.md`
- Modify: `docs/style-guide.md`
- Modify: `CONTRIBUTING.md`

**Interfaces:**

- Consumes: Built CLI output, protocol types, runtime configuration, measured data, architecture invariants and hardware findings.
- Produces: Exact lookup pages and concise explanations of Aurora's two central designs.

- [ ] **Step 1: Write CLI reference from the built binary**

Use these verified commands as the source:

```bash
./result/bin/aurora --help
./result/bin/aurora set --help
./result/bin/aurora load-profile --help
./result/bin/aurora custom-effect --help
```

List every command, the `set` options, the 13 effect names, exit
expectations and three short examples. Use version `0.22.0` only where
an example needs a version.

- [ ] **Step 2: Write runtime-files reference**

List the control socket, settings file, service unit, udev rule,
AppImage log and legacy settings migration sources. Mark paths that
depend on `$XDG_RUNTIME_DIR`, `$XDG_CONFIG_HOME` or the install method.

- [ ] **Step 3: Write architecture explanation**

Explain why Aurora uses a persistent daemon and transient clients.
Describe the daemon core module, bounded command interface, engine,
driver, protocol seam, GUI adapter and CLI adapter. Use one small
Mermaid flow diagram. Explain the leverage and locality of the core
interface without proposing new interfaces.

- [ ] **Step 4: Write Fn+Space synchronization explanation**

Explain:

- why the EC counter is trusted only at acquisition;
- why later HID writes make counter polling observe Aurora's own noise;
- why every WMI event advances the logical sequence;
- why one 250 ms quiet deadline coalesces writes but not key events;
- why one complete feature report avoids stale intermediate states.

Link claims to `docs/research/ite8295-hardware-profiles.md`.

- [ ] **Step 5: Tighten existing reference pages**

Keep `docs/protocol.md` complete. Update stale `0.21.0` examples to
`0.22.0`, shorten its introduction and link slot semantics to the new
explanation.

Keep every measurement and style rule. Remove repeated framing, use
current documentation links and preserve all numbers.

Update `CONTRIBUTING.md` so workflow links point to the source-build
guide, documentation index, measurements and style guide.

- [ ] **Step 6: Check and commit reference and explanation**

Run:

```bash
git diff --check
for file in CONTRIBUTING.md docs/protocol.md docs/measurements.md docs/style-guide.md docs/reference/*.md docs/explanation/*.md; do
  /var/lib/community-skills/waza/skills/write/scripts/check-punctuation.sh --lang en "$file"
done
```

Expected: both commands exit 0.

Then commit:

```bash
git add CONTRIBUTING.md docs/protocol.md docs/measurements.md docs/style-guide.md docs/reference/cli.md docs/reference/runtime-files.md docs/explanation/architecture.md docs/explanation/fn-space-sync.md
git commit -m "docs: add concise reference and explanation"
```

### Task 4: Verify the full migration

**Files:**

- Modify only when a verification failure identifies a documentation error.

**Interfaces:**

- Consumes: Every maintained Markdown page.
- Produces: A linked, build-green documentation branch with no accidental files.

- [ ] **Step 1: Check all local Markdown links**

Use a read-only standard-library link checker over every tracked
Markdown file. Ignore `http`, `https`, `mailto` and image data URLs.
Resolve relative links from each source file and strip anchors before
checking the target path.

Expected: zero missing local targets.

- [ ] **Step 2: Check prose and repository rules**

Run:

```bash
for file in README.md CONTRIBUTING.md docs/README.md docs/quick-start.md docs/install-with-ai.md docs/how-to/*.md docs/reference/*.md docs/explanation/*.md docs/protocol.md docs/measurements.md docs/style-guide.md; do
  /var/lib/community-skills/waza/skills/write/scripts/check-punctuation.sh --lang en "$file"
done
git diff dev...HEAD --check
```

Expected: both commands exit 0.

- [ ] **Step 3: Check commands and build**

Run:

```bash
./result/bin/aurora --help
./result/bin/aurora set --help
nix build
```

Expected: CLI help exits 0 and `nix build` exits 0.

- [ ] **Step 4: Check history and preserved files**

Run:

```bash
git status --short --branch
git log --oneline --decorate --graph -12
git merge-base --is-ancestor dev HEAD
```

Expected:

- current branch is `docs/diataxis-cleanup`;
- `docs/research/mature-daemon-native-gui-references.md` is the only
  untracked file;
- `dev` is an ancestor of the documentation branch;
- no push has occurred.

- [ ] **Step 5: Commit verification corrections if needed**

Stage only files changed to fix failed checks, then commit:

```bash
git commit -m "docs: finish Diataxis migration"
```

Skip this commit when every check passes without further edits.
