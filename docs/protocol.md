# Aurora IPC protocol

This reference defines the interface between the daemon and every
client. It is enough to implement a client without reading Rust. The
types live in [`protocol/src/ipc.rs`](../protocol/src/ipc.rs). If code
and this page disagree, this page is wrong.

Protocol version: **3** (`PROTOCOL_VERSION` in the protocol crate).

Version 2 moved the lighting configuration out of `Profile` and into
`Profile.slots`, one entry per Fn+Space slot. A version 1 client would
misread every profile it received, so this is a breaking change and both
reference clients stop on a mismatch.

Version 3 added the battery features. `DaemonState` gained
`battery_available`, `battery_alert`, `battery_alert_active` and
`battery_percent`, all required, so a version 3 client cannot parse the
state a version 2 daemon sends. That is what makes the change breaking
rather than additive. `Effects` also gained `Battery`, which a version 2
daemon cannot run.

## Transport

- Unix domain socket, stream mode.
- Path: `$XDG_RUNTIME_DIR/aurora.sock`, falling back to
  `/tmp/aurora.sock` when `XDG_RUNTIME_DIR` is unset or empty.
- Encoding: JSON lines. Every message is one JSON object on one line,
  terminated by `\n`. UTF-8.
- Maximum line length: **1 MiB** (`MAX_LINE_BYTES`, 1024 * 1024 bytes),
  in both directions. The daemon disconnects a client that exceeds it;
  clients should treat an oversized server line the same way.
- Empty lines are ignored.

## Envelopes

Clients send request envelopes:

```json
{"id": 1, "req": {"type": "GetState"}}
```

The daemon answers each request with exactly one response envelope
carrying the same `id`:

```json
{"id": 1, "resp": {"type": "State", "state": { ... }}}
```

After a `Subscribe` request, the daemon also pushes event envelopes on
the same connection, interleaved with responses:

```json
{"event": {"type": "StateChanged", "state": { ... }}}
```

Rules:

- `id` is chosen by the client and echoed back verbatim. Use ids >= 1:
  the daemon answers a line it could not parse at all with `id: 0`,
  so 0 means "unattributable".
- Requests on one connection are answered in order. A client may
  pipeline requests and match responses by id.
- Responses and events are distinguished by their top-level field:
  `resp` (with `id`) or `event`.

## Connection lifecycle

1. Connect to the socket.
2. Send `Hello` (recommended; see Versioning below).
3. Send any requests. Send `Subscribe` if you want push updates.
4. Disconnect whenever; the daemon cleans up per-connection state.

The daemon serves any number of concurrent connections. There is no
authentication: the socket lives in the user's runtime directory and
file permissions are the boundary.

## Versioning

Two version numbers exist:

- **Protocol version** (integer): the schema in this document. Bumped
  only on breaking changes (renamed fields, removed variants, changed
  semantics). Additive changes do not bump it.
- **Daemon version** (string): the package version, also present in
  every `DaemonState` as `version`.

Handshake: send `Hello` first on every new connection.

```json
{"id": 1, "req": {"type": "Hello", "protocol_version": 3}}
{"id": 1, "resp": {"type": "Hello", "protocol_version": 3, "daemon_version": "0.24.1"}}
```

- The daemon always answers `Hello` with its own versions, even on
  mismatch (it logs a warning); whether to proceed is the client's
  decision. The reference clients refuse to continue on mismatch.
- A version 1 `Hello` still parses, so the daemon can answer with a
  version mismatch rather than an unhelpful parse error.
- A daemon older than protocol 1 does not know `Hello` and answers
  `{"id": 0, "resp": {"type": "Error", "kind": "InvalidRequest", ...}}`.
  Clients should report that as "daemon predates the handshake", not as
  a protocol failure.
- Unknown request types are always answered with an `InvalidRequest`
  error; the connection stays open.

## Requests

Requests are objects tagged by `"type"`; parameters are sibling fields.

| Type | Parameters | Success response | Description |
| --- | --- | --- | --- |
| `Hello` | `protocol_version` | `Hello` | Version handshake; see above. |
| `GetState` | none | `State` | Full daemon state snapshot. |
| `SetProfile` | `profile` | `Ok` | Make `profile` the live profile, all slots at once, and apply the active slot. Stops a playing custom effect. The profile does not need a name. |
| `SetLighting` | `slot`, `lighting` | `Ok` | Replace one slot's lighting in the live profile and apply it if that slot is active. `slot` of `null` targets whichever slot is active. |
| `SelectSlot` | `slot` | `Ok` | Make `slot` the live position and apply it. |
| `PlayCustomEffect` | `effect` | `Ok` | Play a custom effect until stopped or replaced. |
| `PlayCustomEffectByName` | `name` | `Ok` | Play a saved custom effect without sending its body back. |
| `StopCustomEffect` | none | `Ok` | Stop the playing custom effect and re-apply the active slot. |
| `AddProfile` | `profile` | `Ok` | Save a named profile; overwrites a saved profile with the same name. Name required, non-empty, and at most 64 bytes. |
| `DeleteProfile` | `name` | `Ok` | Delete the saved profile called `name`. |
| `SwitchProfile` | `name` | `Ok` | Make the saved profile called `name` the live profile. |
| `CycleProfile` | none | `Ok` | Advance to the next saved profile, wrapping around. |
| `AddCustomEffect` | `effect` | `Ok` | Save a named custom effect; overwrites one with the same name. Name required and non-empty. |
| `DeleteCustomEffect` | `name` | `Ok` | Delete the saved custom effect called `name`. |
| `SetBatteryAlert` | `enabled` | `Ok` | Turn the low-battery alert on or off. Accepted and stored on a machine with no battery, where it never fires. |
| `Subscribe` | none | `Ok` | Push a `StateChanged` event on this connection whenever daemon state changes. |
| `Shutdown` | none | `Ok` | Ask the daemon to exit cleanly. The `Ok` is queued before exit, but clients should tolerate the connection closing without it. |

There is no request that lists profiles or custom effects. Every state
snapshot already carries their names, so a separate listing round trip
would only offer a second, staler answer.

`SelectSlot` moves Aurora's own slot number, the same one Fn+Space
moves. It cannot drive the controller's counter, because this hardware
exposes no such command. See
[Fn+Space synchronization](explanation/fn-space-sync.md).

Examples:

```json
{"id": 2, "req": {"type": "SwitchProfile", "name": "gaming"}}
{"id": 3, "req": {"type": "SelectSlot", "slot": "Second"}}
{"id": 4, "req": {"type": "SetLighting", "slot": null, "lighting": {"rgb_zones": [{"rgb": [255, 0, 0], "enabled": true}, {"rgb": [0, 255, 0], "enabled": true}, {"rgb": [0, 0, 255], "enabled": true}, {"rgb": [255, 255, 255], "enabled": true}], "effect": "Static", "direction": "Left", "speed": 1, "brightness": "Low"}}}
```

## Responses

Responses are objects tagged by `"type"`.

| Type | Fields | Meaning |
| --- | --- | --- |
| `Hello` | `protocol_version`, `daemon_version` | Handshake answer. |
| `Ok` | none | Request done. |
| `State` | `state` | A `DaemonState` object. |
| `Error` | `kind`, `message` | Request failed; see error kinds. |

## Events

| Type | Fields | Meaning |
| --- | --- | --- |
| `StateChanged` | `state` | Full `DaemonState` snapshot after any change. |

Subscription semantics:

- Events are full snapshots. Replace local state; there is no
  incremental sync.
- The per-connection outbound queue holds 64 lines. A subscriber that
  falls further behind is dropped by the daemon without notice; a
  client that sees its connection die should reconnect, `Subscribe`
  and `GetState` again.
- Events carry no `id` and never answer a request.

## Error kinds

`kind` is one of:

| Kind | Meaning |
| --- | --- |
| `KeyboardNotFound` | No supported keyboard is connected. |
| `PermissionDenied` | A keyboard exists but the daemon may not open it (udev rule missing). |
| `NoSuchProfile` | No saved profile with that name. |
| `NoSuchCustomEffect` | No saved custom effect with that name. |
| `InvalidRequest` | Unparseable line, unknown request type, or a parameter out of range; `message` says which. |
| `Internal` | Anything else; `message` has details. |

## Data types

### DaemonState

```json
{
  "keyboard": {"type": "Connected"},
  "current": { Profile },
  "active_slot": "First",
  "custom_effect_playing": "pulse",
  "profiles": [{"name": "gaming"}],
  "custom_effects": [{"name": "pulse", "step_count": 12, "should_loop": true}],
  "version": "0.24.1",
  "settings_error": null,
  "slot_sync": {"type": "Active"},
  "hotkey": {"type": "Unavailable", "reason": "no display connection"},
  "screen_capture": {"type": "Inactive"},
  "battery_available": true,
  "battery_alert": true,
  "battery_alert_active": false,
  "battery_percent": 53
}
```

- `current` is the live profile, all three slots. The keyboard shows
  `current.slots[active_slot]` unless a custom effect is playing or
  `active_slot` is `"Off"`.
- `active_slot` is a `SlotSelection`. See below.
- `custom_effect_playing` is the playing custom effect's display name,
  or `null` when none plays.
- `profiles` and `custom_effects` carry summaries, not bodies. A
  broadcast holding every profile and every effect body would grow past
  `MAX_LINE_BYTES` and disconnect every client, on every broadcast. Use
  `SwitchProfile` and `PlayCustomEffectByName` to act on a summary.
- `settings_error` is why the last settings write failed, or `null`.
  Lighting keeps working when persistence does not, so a failure is
  reported rather than fatal.
- `slot_sync`, `hotkey` and `screen_capture` are `SubsystemState`
  values. See below.
- `battery_available` says whether this machine has a battery at all. It
  is decided once at daemon startup and fixed for the life of the
  process. Clients hide the low-battery alert setting when it is false:
  there is nothing to configure and nothing the user could act on.
- `battery_alert` says whether the low-battery alert may take the
  keyboard.
- `battery_alert_active` says whether the alert is taking the keyboard
  right now. It is false while `active_slot` is `"Off"`, however low the
  battery, because there is nothing lit to turn red. While it is true the
  keyboard shows red instead of the active slot's lighting, and `current`
  still reports the lighting the user chose. The alert is not a profile
  and is never saved into one, so a client that draws the keyboard must
  consult this field or it will claim a look the hardware is not
  showing.
- `battery_percent` is the charge last read, 0 to 100, or `null` on a
  machine with no battery and before the first read. It is here so a
  client drawing the keyboard can draw the `Battery` effect's gauge; the
  daemon reads the charge itself and does not need this field. Sampled
  every few seconds, so it is recent rather than instantaneous.

At most 128 profiles and 128 custom effects are stored
(`MAX_SAVED_PROFILES`, `MAX_SAVED_CUSTOM_EFFECTS`).

### SlotSelection

One of `"First"`, `"Second"`, `"Third"`, `"Off"`. The controller cycles
three lit slots and an off position, so the type is closed: an
out-of-range slot cannot be represented, let alone sent.

`"Off"` holds no lighting. `SetLighting` targeting it is rejected.

After the daemon acquires the keyboard this is Aurora's own number, not
a live controller reading. Aurora's own lighting writes move the
controller's counter without raising the Fn+Space event, so the counter
is trusted exactly once, at acquisition. See
[Fn+Space synchronization](explanation/fn-space-sync.md).

### SubsystemState

Parts of Aurora that depend on something a machine may not have report
their own state instead of failing the daemon. Tagged by `"type"`:

| Type | Fields | Meaning |
| --- | --- | --- |
| `Active` | none | Working. |
| `Degraded` | `reason` | Working, but something was missed and this state may be wrong. |
| `Unavailable` | `reason` | Not available on this machine or in this session. |
| `Inactive` | none | Available but not running right now. |
| `Unknown` | none | Not determined yet; the state at startup. |

`slot_sync` is Fn+Space detection over the ACPI netlink socket. When it
is not `Active`, slots still work; they have to be selected with
`SelectSlot` rather than cycled with the key. `screen_capture` is
`Inactive` except while the Ambient effect plays.

A client that hides a feature when its subsystem is unavailable should
show the `reason`. It is what a user can act on or report.

### KeyboardStatus

Tagged by `"type"`:

| Type | Fields | Meaning |
| --- | --- | --- |
| `Connected` | none | Keyboard acquired; effects are applied. |
| `Searching` | none | No keyboard found yet; the daemon retries with backoff. |
| `PermissionDenied` | `message` | Keyboard present but not openable (udev rule missing). |
| `Error` | `message` | Any other device failure. |

### Profile

A profile is the named, saveable thing. It owns one `Lighting` per
Fn+Space slot, so switching slots on the keyboard moves between three
looks that belong to the same profile.

```json
{
  "name": "gaming",
  "slots": [ Lighting, Lighting, Lighting ]
}
```

- `name`: string or `null`. Required for `AddProfile`, where it must be
  non-empty and at most 64 bytes (`MAX_NAME_BYTES`).
- `slots`: exactly 3 entries, in the order Fn+Space walks them. A new
  profile starts red, green, and blue.

This shape is what version 2 changed. In version 1 a profile *was* one
lighting configuration, with the fields now inside `Lighting` sitting
directly on the profile, and per-slot lighting lived in a separate
settings field.

### Lighting

What one slot shows.

```json
{
  "rgb_zones": [
    {"rgb": [255, 0, 0], "enabled": true},
    {"rgb": [0, 255, 0], "enabled": true},
    {"rgb": [0, 0, 255], "enabled": false},
    {"rgb": [255, 0, 255], "enabled": true}
  ],
  "effect": "Static",
  "direction": "Left",
  "speed": 3,
  "brightness": "Low"
}
```

- `rgb_zones`: exactly 4 zones, left to right. `rgb` is `[r, g, b]`,
  each 0 to 255. A disabled zone renders black.
- `direction`: `"Left"` or `"Right"`. Only meaningful for effects that
  take a direction (see the effects table); always present.
- `speed`: integer 1 to 10; anything outside that is rejected with
  `InvalidRequest`. Only meaningful for effects that take a speed;
  always present. The four hardware effects accept 1 to 4, and the
  daemon clamps into that range rather than rejecting the request.
- `brightness`: `"Low"` or `"High"`.

### Effects

Unit effects are plain strings; parameterized effects are single-key
objects (externally tagged):

```json
"Static"
{"AmbientLight": {"fps": 30, "saturation_boost": 0.5}}
{"SmoothWave": {"mode": "Change", "clean_with_black": false}}
{"Swipe": {"mode": "Fill", "clean_with_black": true}}
```

| Effect | Parameters | Uses colors | Uses direction | Uses speed |
| --- | --- | --- | --- | --- |
| `Static` | none | yes | no | no |
| `Breath` | none | yes | no | yes |
| `Smooth` | none | no | no | yes |
| `Wave` | none | no | yes | yes |
| `Lightning` | none | yes | no | yes |
| `AmbientLight` | `fps` (1 to 60), `saturation_boost` (0.0 to 1.0) | no | no | no |
| `SmoothWave` | `mode`, `clean_with_black` | no | yes | yes |
| `Swipe` | `mode`, `clean_with_black` | yes | yes | yes |
| `Disco` | none | no | no | yes |
| `Christmas` | none | no | no | no |
| `Fade` | none | yes | no | yes |
| `Temperature` | none | no | no | no |
| `Ripple` | none | yes | no | yes |
| `Battery` | none | yes | no | no |

`mode` is `"Change"` or `"Fill"`.

`Static`, `Breath`, `Smooth` and `Wave` run on the keyboard hardware;
everything else is driven by the daemon. This does not affect the
protocol, but hardware effects survive a daemon stop.

`Battery` turns the keyboard into a charge gauge. It picks no colours of
its own: it takes the slot's four zone colours and dims them from the
right as the battery drains, one zone per 25 percentage points, with the
zone on the charge line dimmed to the fraction of it that is left. The
daemon rejects it with `InvalidRequest` on a machine where
`battery_available` is false, whether it arrives in a `SetLighting`, a
`SetProfile` or an `AddProfile`, so a profile file written on a laptop
does not start a gauge on a desktop. Clients should leave it out of an
effect picker when `battery_available` is false.

### CustomEffect

```json
{
  "name": "pulse",
  "effect_steps": [
    {
      "rgb_array": [255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0],
      "step_type": "Set",
      "brightness": 1,
      "steps": 0,
      "delay_between_steps": 0,
      "sleep": 500
    },
    {
      "rgb_array": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
      "step_type": "Transition",
      "brightness": 1,
      "steps": 50,
      "delay_between_steps": 10,
      "sleep": 0
    }
  ],
  "should_loop": true
}
```

- `name`: string or `null`. Required (non-empty) only for
  `AddCustomEffect`.
- `effect_steps`: 1 to 4096 steps. Empty and oversized lists are
  rejected with `InvalidRequest`.
- `rgb_array`: 12 bytes, 4 zones times `[r, g, b]`, left to right.
- `step_type`: `"Set"` applies the colors at once, `"Transition"` fades
  to them over `steps` increments with `delay_between_steps`
  milliseconds between increments.
- `brightness`: 1 (low) or 2 (high).
- `sleep`: milliseconds to hold after the step.
- `should_loop`: restart from the first step after the last.

## Example session

```text
C: {"id":1,"req":{"type":"Hello","protocol_version":3}}
S: {"id":1,"resp":{"type":"Hello","protocol_version":3,"daemon_version":"0.24.1"}}
C: {"id":2,"req":{"type":"Subscribe"}}
S: {"id":2,"resp":{"type":"Ok"}}
C: {"id":3,"req":{"type":"GetState"}}
S: {"id":3,"resp":{"type":"State","state":{...}}}
C: {"id":4,"req":{"type":"SelectSlot","slot":"Second"}}
S: {"event":{"type":"StateChanged","state":{...}}}
S: {"id":4,"resp":{"type":"Ok"}}
```

Note the event can arrive before the response that caused it; match on
`id` and `resp`/`event`, never on ordering.
