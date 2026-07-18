# Aurora house rules

Rules for anyone (human or coding agent) changing this repo. They exist
because the owner reads every diff; optimize for legible data flow, not
line count.

## Style: TigerStyle, adapted to Rust

Based on [TigerBeetle's TIGER_STYLE](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md).

- **No clever one-liners.** When data moves (parse, copy, transform,
  fan-out), write the loop with named intermediate variables so the
  movement is visible. A small `map` or `filter` is fine; dense chains
  and point-free style are not.
- **Assert invariants liberally.** Preconditions, postconditions and
  ranges (zone count is 4, speed within range, payload length exact).
  `debug_assert!` on hot paths, `assert!` at boundaries. Never assert on
  peer input; reject it with an error instead.
- **Bound everything.** Channels, queues, retries and line lengths get
  named capacity constants (`MESSAGE_QUEUE_CAPACITY`, `MAX_LINE_BYTES`).
  No unbounded channels.
- **Units and types in names**: `retry_delay_ms`, `debounce_secs`,
  `window_secs`.
- **Short, flat functions.** Around 70 lines maximum, control flow flat,
  no recursion.
- **No `unwrap`/`expect` on daemon paths.** Every driver or IO error is
  handled or surfaced as `KeyboardStatus`/`Response::Error`. The GUI may
  `expect` only on programmer-error invariants.

## Architecture invariants

- The daemon core loop is the single owner of daemon state: only the
  core thread mutates it; everything else (IPC clients, hotkey, tray)
  sends `Command` messages.
- `protocol/` stays UI-free and IO-free: types and schema only.
- The GUI and CLI never touch the settings file; all state flows through
  the daemon socket.
- The GUI uses stock libadwaita widgets and follows the GNOME HIG.
  Custom drawing is confined to the keyboard preview widget. Widget
  updates are compare-before-set (this is also the echo-loop guard).
- `driver/` keeps its upstream name and shape as credit to
  4JX/L5P-Keyboard-RGB; change it only with a clear hardware reason.

## Process

- Conventional commits (`type(scope): imperative summary`), no AI
  attribution trailers.
- `nix build` must pass before push; the committed `hooks/pre-push`
  enforces this (`git config core.hooksPath hooks` once per clone).
- No em dashes in docs or user-facing strings.
- Docs stay short: digest measurements and decisions, link to details.
