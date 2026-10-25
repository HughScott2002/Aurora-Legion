# True Pixel

Real renders of the app, captured headlessly, for checking the Figma
kit against. The Figma file lives at
<https://www.figma.com/design/LjD1VqORncmYFXGqpzpcXc> (pages: the kit,
the screens, and a ground-truth page holding the captures themselves).

## Why this exists

The Figma kit is matched against rendered pixels, not against the
stylesheets. Both were tried, and the stylesheets lost. A render is the
thing the user sees; a stylesheet is one input to it, and libadwaita
does surprising things on the way to the screen. Found by capture and
contradicted by CSS reading:

- the header bar is flat and transparent in libadwaita 1.8, window
  background showing through, no solid band, no shade line
- preference cards render 513px wide at x=63, because
  `AdwPreferencesGroup` carries its own 12px internal margins inside
  the 560px clamp
- the view switcher's selected pill is a plain 10% control fill on the
  selected tab only; unselected tabs are bare text with no group
  background at all
- content starts 25px below the header bar, not the 18px the layout
  code asks for
- the accent colour that actually renders is the live gsettings value
  (`#3380DD` here), not the stylesheet's `#3584E4`

## What is in here

- `capture.sh` drives the installed `aurora-gui` under Xvfb and saves
  nine captures: the four pages, the backlight-off state, the menu
  popover, the About dialog, the Save Profile dialog, and the
  disconnected status page. Every click is followed by a pixel check,
  so a moved widget fails the run instead of quietly capturing the
  wrong thing.
- `rowruns.sh` measures a capture one row at a time, printing where
  each surface starts and ends. This is how the click coordinates were
  found, and how to re-find them when the UI moves.
- `diff.sh` scores a Figma export against a capture: one number for any
  difference, one for real drift.

## Running it

```
nix develop ./labs/true-pixel -c bash labs/true-pixel/capture.sh
```

Append `connecting` to capture only the disconnected status page.

The flake shell provides every tool the captures call by name
(xdotool, Xvfb, ImageMagick, fontconfig), pinned to the same nixpkgs
revision as the project itself, so nothing needs installing and the
tools cannot drift from what the app was built against. The app comes
from the profile rather than the flake, on purpose: the harness
captures what is installed and running, not what the working tree says.

Also required: the daemon running (`systemctl --user start aurora`).
Captures land in `/tmp/aurora-true-pixel/out`; set `WORK` to put them
elsewhere. `rowruns.sh` and `diff.sh` want the same shell, so prefix
them the same way.

Measuring a moved widget:

```
nix develop ./labs/true-pixel -c labs/true-pixel/rowruns.sh /tmp/aurora-true-pixel/out/01-lighting.png 23
```

Scoring the kit:

```
nix develop ./labs/true-pixel -c labs/true-pixel/diff.sh settings-export.png /tmp/aurora-true-pixel/out/04-settings.png
```

## What it does to the machine

Nothing that outlives the run. The slot selection is saved and
restored (the backlight-off capture does briefly turn the real
keyboard off), no settings are written, and the capture display is
`:77` because a display `:99` on this machine belongs to someone else.
The connected screens talk to the running daemon read-only; the
disconnected screen just hides the socket from the app.

## The honest limit

Text pixels cannot match between a GTK render and any design tool:
Pango and Figma rasterize glyphs differently even with the same
typeface, and Figma does not carry Adwaita Sans anyway (the kit
substitutes Inter, its upstream). Flat surfaces, geometry and colour
can match exactly, and after the 0.25.0 pass they do. `diff.sh` prints
both numbers so the text remainder is never mistaken for drift.
