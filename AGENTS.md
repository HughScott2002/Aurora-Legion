# Aurora agent rules

Always in force:

- No clever one-liners: data movement is explicit loops with named
  intermediates. Small `map`/`filter` fine; nothing dense or point-free.
- No `unwrap`/`expect` on daemon paths; surface errors via the protocol.
- Bound every channel, queue and retry with a named constant; prefer
  blocking waits over polling.
- Only the daemon core thread mutates daemon state; `protocol/` stays
  UI-free; GUI/CLI never touch the settings file.
- Conventional commits, no AI attribution trailers, no em dashes in
  docs or user-facing strings. `nix build` must pass before push.

Fetch details when the task needs them:

- Writing or reviewing Rust/GTK code: read `docs/style-guide.md` first
  (full TigerStyle rules, architecture invariants, GUI rules).
- Committing, building locally or opening a PR: read `CONTRIBUTING.md`
  (hook setup, devshell flags, commit format, measurement expectations).
