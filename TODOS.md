# TODOS

Working backlog. Durable detail lives in the linked issues; keep this
file to agent-facing summaries and priority.

## Fn+Space hardware slot sync (branch `feat/hardware-profile-sync`)

Happy path works end to end on the 2023 Pro; parked with known bugs.
Full state and analysis: #14 (comment dated 2026-07-25).

- [ ] Seed distinct per-slot default colors (red/green/blue) and verify
      the read-and-apply path end to end before building slot color
      editing. Issue: #14
- [ ] Daemon: slot switches sometimes skip; suspected self-write
      feedback into queued Fn+Space events. Issue: #14
- [ ] GUI: rethink the state-update path so preview, zone pickers and
      slot caption always follow daemon broadcasts. Issue: #14
- [ ] GUI: relm4 panic (`Ipc(Disconnected)` into a shut-down runtime)
      when the daemon connection drops; must degrade, not crash.
      Issue: #14
