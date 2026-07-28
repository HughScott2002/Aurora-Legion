# Interface style guide

Rules for what Aurora's app looks like, in the same spirit as
[the code style guide](style-guide.md). Read this before changing the GUI.

Three sources, used for different things:

- [GNOME HIG](https://developer.gnome.org/hig/principles.html) for what
  the platform expects. Aurora is a GNOME app and should not argue with
  its host.
- [Refactoring UI](https://www.sglavoie.com/posts/book-summary-refactoring-ui/)
  for the visual craft the HIG does not teach: hierarchy, spacing,
  de-emphasis.
- [Rams' ten principles](https://designmanifestos.org/dieter-rams-ten-principles-for-good-design/)
  for taste. "As little design as possible" is the same instinct as
  TigerStyle's "delete the part".

The rules below are what those three amount to for this project.

## The keyboard is the display

Never render on screen what the hardware shows better 30 cm away. The
lit keys are the real output, at a fidelity no widget can match, and a
duplicate on screen is always the worse copy.

The keyboard preview earns its place only when the laptop is not in
front of you or the backlight is off. It is a reference, not the point
of the page. Colour chips beside a slot picker are not a reference; they
compete with the thing they describe. That is why they are gone.

This rule is Aurora's own. No general guide would produce it, because
most apps do not sit next to their output device.

## Not everything can be prominent

Pick one lever per element: bigger, or bolder, or brighter. Never all
three, and never for more than one element in a region.

De-emphasise secondary content rather than emphasising everything.
Captions get `caption` and `dim-label`. A label that explains a control
is secondary to the control.

If two things on a page compete for the eye, one of them is wrong.

## Space comes from a scale

6, 12, 18, 24. Nothing else, unless a stock widget dictates it.

Start with too much space and remove it. Dense is easy to reach by
accident and hard to recover from.

## One job per region

A group that needs a sentence to explain what it is for is doing two
jobs. Split it or cut one.

The control a page exists for goes at the top. On the Lighting page that
is the slot picker, because every other control on the page edits the
selected slot. A page you have to read bottom-up to understand is
ordered wrong.

## Say the true thing, including bad news

When a feature cannot work on this machine, say so where the feature
would have been used, with the reason. Silence reads as "working".

This is the interface half of the daemon's subsystem states. The daemon
knows Fn+Space is unavailable; the slot picker is where that has to
appear.

## Stock widgets

Stock libadwaita only. Custom drawing is confined to the keyboard
preview, which is the one thing no stock widget can be.

Target libadwaita 1.5 and GTK 4.14, the Ubuntu 24.04 LTS baseline, as
[`gui/Cargo.toml`](../gui/Cargo.toml) records. Newer widgets are not
available: `adw::ToggleGroup` needs 1.7, so linked `gtk::ToggleButton`s
do that job.

Widget updates are compare-before-set. This is a correctness rule as
much as a style one: it is what stops signal echo loops.

## Hide what does not apply, do not grey it out

A greyed control still occupies the page, still has to be read, and still
raises the question it cannot answer: what would this do if I could reach
it? Remove it instead. Direction means nothing under Static, so under
Static there is no Direction row.

Grey out only when the control will become reachable through something
the user is about to do, and they need to see it coming. "This effect
does not have that setting" is not that case.

Widgets built hidden must start hidden. State arrives after the window
does, so anything the default does not use is on screen until the first
update, and a row that flashes in and out is worse than one that stays.

Compare with `get_visible`, never `is_visible`: the latter is false
whenever any ancestor is hidden, so the compare-before-set skips the
write and the widget keeps a stale flag until it is shown again.

## Prefer deleting a control to explaining it

If a control needs a tooltip to justify existing, try removing it and
see who complains.

Aurora's users came to light their keyboard. Anything between them and
that is overhead, and overhead is measured in controls, not pixels.
