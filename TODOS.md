# TODOS

Working backlog. Durable detail lives in the linked issues; keep this
file to agent-facing summaries and priority.

## Sharing profiles and custom effects

Loading a custom effect from a file is the only way to get one, and
nothing writes such a file. Export comes before in-app authoring. Full
ordering: #17.

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
