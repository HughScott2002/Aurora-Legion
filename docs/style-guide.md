# Style guide

The long-form rules behind the invariants in [AGENTS.md](../AGENTS.md).
Read this before writing or reviewing Rust or GTK code in this repo.

## TigerStyle, adapted to Rust

Based on [TigerBeetle's TIGER_STYLE](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md).

- **No clever one-liners.** When data moves (parse, copy, transform,
  fan-out), write the loop with named intermediate variables so the
  movement is visible. A small `map` or `filter` is fine; dense chains
  and point-free style are not. The owner reads every diff; optimize
  for legible data flow, not line count.
- **Assert invariants liberally.** Preconditions, postconditions and
  ranges (zone count is 4, speed within range, payload length exact).
  `debug_assert!` on hot paths, `assert!` at boundaries. Never assert on
  peer input; reject it with an error instead.
- **Bound everything.** Channels, queues, retries and line lengths get
  named capacity constants (`MESSAGE_QUEUE_CAPACITY`, `MAX_LINE_BYTES`).
  No unbounded channels. Blocking waits over polling loops; when a poll
  is unavoidable (device_query), document why at the constant.
- **Units and types in names**: `retry_delay_ms`, `debounce_secs`,
  `window_secs`.
- **Short, flat functions.** Around 70 lines maximum, control flow flat,
  no recursion.
- **No `unwrap`/`expect` on daemon paths.** Every driver or IO error is
  handled or surfaced as `KeyboardStatus`/`Response::Error`. The GUI may
  `expect` only on programmer-error invariants.

## Architecture invariants

- The daemon core loop is the single owner of daemon state: only the
  core thread mutates it; everything else (IPC clients, hotkey, signal
  listener) sends `Command` messages.
- `protocol/` stays UI-free and IO-free: types and schema only.
- The GUI and CLI never touch the settings file; all state flows through
  the daemon socket.
- `driver/` keeps its upstream name and shape as credit to
  4JX/L5P-Keyboard-RGB; change it only with a clear hardware reason.

## GUI rules

- Stock libadwaita widgets only; follow the GNOME HIG.
- Custom drawing is confined to the keyboard preview widget.
- Widget updates are compare-before-set; this doubles as the guard
  against signal echo loops.
- Long-running work (systemctl calls, file IO) never runs on the GTK
  main loop; spawn a thread and deliver results as messages.
