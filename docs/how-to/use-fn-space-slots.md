# Use Fn+Space slots

Aurora remembers one lighting profile for each of three lit Fn+Space
slots. The fourth state is off.

On first use, the slots are:

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
daemon:   running (v0.22.0)
keyboard: connected
hw slot:  1 of 3
```

When the fourth state is active, status prints:

```text
hw slot:  backlight off (Fn+Space)
```

## Change a slot

Press Fn+Space until `aurora status` shows the slot you want. Apply a
profile with the GUI or CLI.

This example makes all four zones purple:

```console
$ aurora set -e Static \
    -c 128,0,255,128,0,255,128,0,255,128,0,255
profile applied
```

Aurora stores the profile in the active lit slot. Switch away and back
to confirm it returns.

## Set each slot

Repeat this sequence:

1. Press Fn+Space once.
2. Wait until `aurora status` shows the intended slot.
3. Apply the profile.

Use a different visible color for each slot while testing. It makes a
missed or extra event obvious.

## Understand off

Off is not a saved lighting slot. A `set` command while status says
`backlight off` lights the keyboard but does not overwrite slots 1
through 3. Press Fn+Space to return to slot 1.

Aurora counts every matching Fn+Space event, then waits 250 ms after
the final event before writing. Rapid taps can show firmware lighting
briefly, but the final Aurora slot should win.

If it does not, follow
[Fn+Space troubleshooting](troubleshoot.md#fnspace-shows-firmware-rgb-or-darkness).

## Current GUI limits

The keyboard and daemon state switch correctly. The GUI preview, slot
caption and color pickers do not always refresh together after a
daemon broadcast. Check `aurora status` before editing when the GUI
looks stale.

This follow-up is tracked in
[issue #14](https://github.com/HughScott2002/Aurora-Legion/issues/14).
