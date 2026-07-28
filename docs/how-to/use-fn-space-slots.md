# Use Fn+Space slots

Every profile holds three lightings, one per lit Fn+Space slot. The
fourth state is off. Switching slots on the keyboard moves between three
looks that belong to the same profile, and saving the profile saves all
three.

A new profile starts:

| Slot | Default |
| --- | --- |
| 1 | Red |
| 2 | Green |
| 3 | Blue |
| 4 | Off |

Existing settings keep their saved colors.

## Check the active slot

```console
$ aurora status
daemon:   running (v0.24.0)
keyboard: connected
slot:     1 of 3 (Static effect)
```

When the fourth state is active, status prints:

```text
slot:     backlight off (Fn+Space)
```

## Change slots

Three ways reach the same place, and all of them apply immediately:

- Press Fn+Space.
- Click a slot in the app, at the top of the Lighting page.
- Run `aurora slot 2`.

```console
$ aurora slot 2
slot 2 selected
```

Aurora moves its own slot number. The hardware exposes no command to
move the controller's counter, so selecting a slot in software and
pressing the key are not quite the same operation, even though they look
identical. [Fn+Space synchronization](../explanation/fn-space-sync.md)
explains why.

## Edit a slot

Select the slot you want, then change its lighting. The app edits the
selected slot, and `aurora set` writes to it:

```console
$ aurora set -e Static \
    -c 128,0,255,128,0,255,128,0,255,128,0,255
lighting applied
```

Switch away and back to confirm it returns.

To write a slot you are not looking at, name it. The keyboard keeps
showing the live slot:

```console
$ aurora set --slot 3 -e Breath -c 0,0,255,0,0,255,0,0,255,0,0,255
lighting applied
```

Use a different visible color per slot while testing. It makes a missed
or extra event obvious.

## Understand off

Off is not a lighting slot. It holds nothing, and editing it is
rejected: a change applied while the backlight is off would light the
keyboard and then vanish at the next restart, because there is nowhere
to store it. Select a lit slot first.

Aurora counts every matching Fn+Space event, then waits 250 ms after the
final event before writing. Rapid taps can show firmware lighting
briefly, but the final Aurora slot should win.

If it does not, follow
[Fn+Space troubleshooting](troubleshoot.md#fnspace-shows-firmware-rgb-or-darkness).

## When Fn+Space is not detected

Detection needs the ACPI event socket, and not every machine or kernel
provides it. Aurora does not pretend otherwise: the app says so under
the slot buttons, and `aurora status` prints the reason.

Slots still work. Select them in the app or with `aurora slot` instead
of pressing the key. If your laptop lands here, the Settings page has a
link to report it, and the model is the useful part of the report.
