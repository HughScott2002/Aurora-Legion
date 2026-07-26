# CLI reference

`aurora` runs the daemon and controls a running daemon. Commands return
0 on success and a nonzero status on failure.

## Commands

| Command | Purpose |
| --- | --- |
| `aurora daemon` | Run the daemon in the foreground. |
| `aurora set` | Build and apply a profile from command-line options. |
| `aurora list` | List the 13 built-in effects. |
| `aurora status` | Show daemon, keyboard, profile and Fn+Space slot state. |
| `aurora cycle-profile` | Apply the next profile saved through the GUI. |
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
| `--save` | Path for a profile JSON file | No file |

Color bytes follow the keyboard from left to right:

```text
R1,G1,B1,R2,G2,B2,R3,G3,B3,R4,G4,B4
```

If no daemon runs, `set` and `load-profile` can apply the four hardware
effects directly. Software effects need the daemon because a process
must keep writing frames.

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
profile applied
```

Run a wave from right to left:

```console
$ aurora set -e Wave -s 3 -d Right
profile applied
```

Inspect the daemon:

```console
$ aurora status
daemon:   running (v0.22.0)
keyboard: connected
profile:  (unsaved) (Static effect)
hw slot:  1 of 3
```
