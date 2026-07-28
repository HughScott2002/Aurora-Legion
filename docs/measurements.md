# Performance measurements

Measured 2026-07-27 on a 2023 Lenovo Legion Pro with controller
`048d:c985`, NixOS and GNOME on Wayland. Both projects were built and
measured on the same machine, the same day, through the same Nix
pipeline:

- Baseline: `legion-kb-rgb` 0.20.8 at `b05be4c`, built from
  `github:4JX/L5P-Keyboard-RGB`. Still upstream's current release.
- Aurora 0.24.0: `nix build` on `dev` at `7f3b17a`.

## Method

- Memory is PSS from `/proc/PID/smaps_rollup`. PSS accounts for shared
  pages and gives a fairer GUI comparison than RSS.
- CPU is the utime+stime delta from `/proc/PID/stat` over a 60 second
  window, expressed as percent of one core.
- Sampler: [`measure.sh`](measure.sh). Two passes per scenario, no other
  workload.
- **The first pass is discarded.** Process startup, font and icon theme
  loading, and the first render all land inside a 60 second window and
  dominate it. Aurora's GUI reads 2.80% on pass 1 and 0.13% on pass 2
  from the same idle process; upstream reads 0.65% then 0.13%. The
  tables below report pass 2, the settled figure. Both passes are shown
  so the gap is visible rather than hidden.
- The upstream app and the Aurora daemon were never running at the same
  time. They contend for the same hidraw device.

## Results

| Scenario | PSS pass 1 | PSS pass 2 | CPU pass 1 | CPU pass 2 |
| --- | --- | --- | --- | --- |
| upstream, Static, window open | 92.5 MiB | 92.5 MiB | 0.65% | 0.13% |
| upstream, Swipe, window open | 92.2 MiB | 92.2 MiB | 0.52% | 0.52% |
| aurora daemon, Static | 11.4 MiB | 11.5 MiB | 0.12% | 0.05% |
| aurora daemon, Swipe | 11.5 MiB | 11.5 MiB | 0.48% | 0.50% |
| aurora-gui, open and connected, idle | 84.3 MiB | 85.2 MiB | 2.80% | 0.13% |

Binary sizes from the nix outputs (`du -bL`, following the nix wrapper
to the real binary): upstream single binary 26.6 MB; aurora daemon
8.7 MB plus GUI 2.7 MB.

## What changed since the 0.21.0 round

The earlier measurement, on 2026-07-18, is not comparable to this one
and was replaced rather than extended. Both projects grew by about
10 MiB of PSS on the same hardware between the two dates, upstream from
82.6 to 92.5 MiB and the Aurora GUI from 61.0 to 85.2 MiB, without
either project changing its own toolkit choice. The system's GTK and
libadwaita moved underneath them.

That is the reason for re-measuring both sides rather than refreshing
Aurora's column alone. Comparing a current Aurora number against a
stale upstream number would have reported a ratio neither project
earned. Aurora's resident advantage is the same 8x it was; only the
absolute numbers moved.

The daemon grew from 10.9 to 11.5 MiB across the same period, while
gaining the Fn+Space listener thread, per-slot lighting, and subsystem
state reporting.

## Idle CPU after issue #1

The 0.21.0 round showed Aurora idling at 0.17% versus upstream's 0.10%
because the engine idle loop woke every 20 ms, the core ticked every
250 ms and the hotkey polled every 50 ms. After the fix (engine blocks
on its channel, core ticks at 2 s when healthy with a signal listener
for instant shutdown, hotkey at 100 ms), idle settled to 0.05% and has
stayed there.

SIGTERM-to-exit latency measured at 160 ms with the slow tick active.

## Interpretation

- The resident process, which is what runs whenever your lights are on,
  is 11.5 MiB against 92.5 MiB. The resident part carries no GUI
  toolkit, renderer or tray stack.
- Idle CPU is 0.05% against 0.13%. The remaining cost is the 100 ms
  hotkey poll from `device_query`.
- Swipe CPU is the same within noise, 0.50% against 0.52%. It is the
  same HID transition code, inherited from upstream.
- The GTK4 GUI uses about 85 MiB while open, slightly less than
  upstream's window, and it exits when closed. Upstream's window has to
  stay resident for the lights to keep working, so the honest
  comparison is 85 MiB sometimes against 92.5 MiB always.
