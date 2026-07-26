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
