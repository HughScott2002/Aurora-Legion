# TODOS

Working backlog. Durable detail lives in the linked issues; keep this
file to agent-facing summaries and priority.

## Fn+Space hardware slot follow-up

Slot sync and distinct red, green and blue defaults work end to end on
the 2023 Pro. Full state and analysis: #14.

- [ ] GUI: rethink the state-update path so preview, zone pickers and
      slot caption always follow daemon broadcasts. Issue: #14
- [ ] GUI: relm4 panic (`Ipc(Disconnected)` into a shut-down runtime)
      when the daemon connection drops; must degrade, not crash.
      Issue: #14

## Sharing profiles and custom effects

Loading a custom effect from a file is the only way to get one, and
nothing writes such a file. Export comes before in-app authoring. Full
ordering: #17.

- [ ] Export a profile or custom effect from the GUI and the CLI.
      Issue: #17
- [ ] Custom Effects empty state should say where a file comes from
      instead of implying the user has one. Issue: #17
- [ ] Build custom effects in the app. Deferred behind export. Issue: #17
