# Measurements

Collected 2026-07-18 on the development machine: Lenovo Legion Pro (2023,
keyboard controller 048d:c985), NixOS, GNOME on Wayland. Both versions
were built by the same nix pipeline (release profile, same toolchain):

- upstream baseline: `legion-kb-rgb` 0.20.8, built from git rev
  `b05be4c` (the last commit before the rearchitecture)
- aurora 0.21.0: `nix build` at `4aca7b0`

## Method

- Memory is PSS read from `/proc/PID/smaps_rollup` (proportional
  accounting of shared pages; fairer than RSS for GUI stacks).
- CPU is the utime+stime delta from `/proc/PID/stat` over a 60 second
  window, expressed as percent of one core.
- Sampler: [`measure.sh`](measure.sh). Two passes per scenario, no other
  workload running. Values rounded against aurora, not in its favor.
- The upstream app and the aurora daemon were never running at the same
  time (they would contend for the same hidraw device).

## Results (two passes each)

| Scenario | PSS pass 1 | PSS pass 2 | CPU pass 1 | CPU pass 2 |
| --- | --- | --- | --- | --- |
| upstream, Static, window open | 82.6 MiB | 82.6 MiB | 0.13% | 0.10% |
| upstream, Swipe, window open | 82.3 MiB | 82.3 MiB | 0.52% | 0.52% |
| aurora daemon, Static | 10.1 MiB | 10.2 MiB | 0.18% | 0.17% |
| aurora daemon, Swipe | 10.2 MiB | 10.8 MiB | 0.97% | 0.55% |
| aurora-gui, open + connected, idle | 61.0 MiB | 60.9 MiB | 0.17% | 0.03% |

Binary sizes from the nix outputs (`du -b`): upstream single binary
26.6 MB; aurora daemon 8.4 MB plus GUI 2.5 MB.

## Reading the numbers honestly

- The resident process (what runs whenever your lights are on) shrinks
  from 82.6 MiB to about 10 MiB, because the resident part no longer
  carries a GUI toolkit, a renderer or a tray stack.
- Idle CPU is *worse* in aurora: about 0.17% versus upstream's 0.10%.
  The daemon's engine idle loop, core tick and hotkey poll all wake on
  timers. Replacing the polls with blocking waits is tracked in the
  issue linked from the README.
- Swipe CPU is comparable with higher variance (0.55 to 0.97% versus a
  steady 0.52%); the work (HID transitions) is the same code inherited
  from upstream.
- The GTK4 GUI uses about 61 MiB while open, which is still less than
  the upstream window's 82.6 MiB, and it exits when closed; upstream's
  window had to stay resident for the lights to keep working.
