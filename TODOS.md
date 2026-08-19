# TODOS

Working backlog. Durable detail lives in the linked issues; keep this
file to agent-facing summaries and priority.

## Sharing profiles and custom effects

Loading a custom effect from a file is the only way to get one, and
nothing writes such a file. Export comes before in-app authoring. Full
ordering: #17.

- [ ] Drop the blanket `*.json` from `.gitignore` first. Both exported
      formats are JSON, so example files would be silently ignored.
      Hides nothing today. Issue: #12
- [ ] Export a profile or custom effect from the GUI and the CLI.
      Issue: #17
- [ ] Custom Effects empty state should say where a file comes from
      instead of implying the user has one. Issue: #17
- [ ] Build custom effects in the app. Deferred behind export. Issue: #17

## Interface polish

Slot picker, hide-do-not-grey, the Settings page and a written style
guide shipped in 0.24.0. What is left of the HIG pass: #4.

- [ ] Accessibility: Orca, keyboard-only navigation, focus order,
      contrast. Not started. Issue: #4
- [ ] Keyboard preview at window sizes other than the default. Issue: #4
- [ ] Profiles and Custom Effects pages have had no HIG pass. Issue: #4
- [ ] `GtkImage` baseline warnings on every list rebuild, from icon
      suffixes in the Profiles and Custom rows. Issue: #4

## Release gate

- [ ] Re-test the README claims against a release candidate. Never run
      for 0.24.0 or 0.24.1, and the comparison table, every performance
      figure, and the install instructions all changed in between.
      Issue: #16
- [ ] Install on a machine with no prior Aurora udev rules, following
      `docs/how-to/install-linux.md` exactly. 0.24.0 shipped rules that
      granted nothing, and the 0.24.1 fix is confirmed only on a machine
      that already had Aurora working. Issue: #16, #20

## Measurement

Numbers were re-run after the hotkey removal and the method did not
survive review. `docs/measure-compare.sh` is committed unfinished, with
its gaps in its own header. Full context: #28.

- [ ] Fix the harness before running it: raw counters in the CSV, paired
      per-round deltas instead of independent medians, even round count,
      schedstat nanoseconds instead of clock ticks. Issue: #28
- [ ] Check by eye whether upstream keeps a static colour lit after it
      exits. If it does, the claim that its window must stay resident is
      overstated, and that claim is in the README. Issue: #28
- [ ] Re-run both projects the same day, both scenarios. Upstream has no
      controls-closed mode, `--hideWindow` is a no-op, so the asymmetry
      is the result. Issue: #28
- [ ] The GUI scenario compares a lone 85 MiB GUI against upstream and
      omits the daemon running alongside it. Issue: #28
- [ ] Per-client writer thread parks on its channel until there is
      something to send, so a client that disconnects against an idle
      daemon leaves it until the next broadcast. Bounded by any state
      change, unbounded in principle. Found while counting threads; no
      issue yet.

## Diagnostics

- [ ] `aurora doctor`: enumerate the device, report whether the udev
      rule is present and whether the ACL actually landed, socket path,
      daemon and client versions. Subsystem states already carry half of
      this. The ACL check is the one #20 was invisible against.
      Issue: #3
