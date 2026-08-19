# CLI reference

`aurora` runs the daemon and controls a running daemon. Commands return
0 on success and a nonzero status on failure.

## Commands

| Command | Purpose |
| --- | --- |
| `aurora daemon` | Run the daemon in the foreground. |
| `aurora set` | Build and apply lighting from command-line options. |
| `aurora list` | List the 13 built-in effects. |
| `aurora status` | Show daemon, keyboard, profile and Fn+Space slot state. |
| `aurora cycle-profile` | Apply the next profile saved through the GUI. |
| `aurora slot` | Show or change the active Fn+Space slot. |
| `aurora load-profile` | Load and apply a profile JSON file. |
| `aurora custom-effect` | Load and play a custom-effect JSON file. |
| `aurora stop` | Stop a custom effect and restore the current profile. |
| `aurora shutdown` | Ask the daemon to exit cleanly. |

Run `aurora <command> --help` for the built-in summary.

## `set`

```text
aurora set --effect <EFFECT> [OPTIONS]
```

| Option | Value | Default |
| --- | --- | --- |
| `-e`, `--effect` | Built-in effect name | Required |
| `-c`, `--colors` | 12 comma-separated bytes, four RGB triplets | Required by color effects |
| `-b`, `--brightness` | `Low` or `High` | `Low` |
| `-s`, `--speed` | 1 to 4 for hardware effects, 1 to 10 for software effects | 1 |
| `-d`, `--direction` | `Left` or `Right` | Effect default |
| `--slot` | `1`, `2` or `3`: the slot to write | The active slot |
| `--save` | Path for a profile JSON file | No file |

`--slot` writes one slot without selecting it, so the keyboard keeps
showing whatever slot is live. Omit it to edit the slot in front of
you.

Color bytes follow the keyboard from left to right:

```text
R1,G1,B1,R2,G2,B2,R3,G3,B3,R4,G4,B4
```

If no daemon runs, `set` and `load-profile` can apply the four hardware
effects directly. Software effects need the daemon because a process
must keep writing frames. `--slot` needs a daemon: without one there is
no stored profile to write a slot into.

## `slot`

```text
aurora slot [SLOT]
```

`SLOT` is `1`, `2`, `3`, or `off`. Omit it to print the active slot.

```console
$ aurora slot
slot 1 of 3

$ aurora slot 2
slot 2 selected
```

Selecting a slot applies it immediately, exactly as pressing Fn+Space
onto it would. Aurora moves its own slot number; the hardware exposes
no command to move the controller's counter. See
[Use Fn+Space slots](../how-to/use-fn-space-slots.md).

## `cycle-profile`

```text
aurora cycle-profile
```

Applies the next saved profile, wrapping at the end. If the current
lighting was never saved, it starts from the first saved profile.

This is meant to be bound to a keyboard shortcut. Aurora does not grab
keys itself, because a daemon that watches the keyboard has to poll and
only sees the keys its display server hands it. The desktop already
does this properly, so bind it there:

Settings, Keyboard, View and Customize Shortcuts, Custom Shortcuts, and
add `aurora cycle-profile` with whatever key you want. The binding works
on Wayland and X11 alike, and survives Aurora being restarted or
upgraded.

## Effects

```text
Static
Breath
Smooth
Wave
Lightning
AmbientLight
SmoothWave
Swipe
Disco
Christmas
Fade
Temperature
Ripple
```

`Static`, `Breath`, `Smooth` and `Wave` run on the keyboard. See
[Effects](../protocol.md#effects) for parameters and color behavior.

## File commands

Load a profile:

```console
$ aurora load-profile --path gaming.json
profile applied
```

Play a custom effect:

```console
$ aurora custom-effect --path pulse.json
custom effect playing
```

`set --save FILE` writes a profile file before applying it. It does not
add the profile to the daemon's named profile list. Save named profiles
through the GUI.

## Examples

Set four zone colors:

```console
$ aurora set -e Static \
    -c 255,0,0,0,255,0,0,0,255,255,255,255
lighting applied
```

Run a wave from right to left:

```console
$ aurora set -e Wave -s 3 -d Right
lighting applied
```

Inspect the daemon:

```console
$ aurora status
daemon:   running (v0.24.1)
keyboard: connected
profile:  (unsaved)
slot:     1 of 3 (Static effect)
saved:    2 profiles (gaming, dim)
```

`status` also prints a line per optional subsystem that is not working,
with the reason. Nothing is printed for a subsystem that works.
