# Aurora IPC protocol

The contract between the Aurora daemon and any client (the GTK GUI, the
CLI, or a third-party frontend). This document is complete on purpose:
a client can be written from it alone, in any language, without reading
the Rust types. The types live in [`protocol/src/ipc.rs`](../protocol/src/ipc.rs);
if this document and the code disagree, that is a bug in this document.

Protocol version: **1** (`PROTOCOL_VERSION` in the protocol crate).

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
{"id": 1, "req": {"type": "Hello", "protocol_version": 1}}
{"id": 1, "resp": {"type": "Hello", "protocol_version": 1, "daemon_version": "0.21.0"}}
```

- The daemon always answers `Hello` with its own versions, even on
  mismatch (it logs a warning); whether to proceed is the client's
  decision. The reference clients refuse to continue on mismatch.
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
| `SetProfile` | `profile` | `Ok` | Make `profile` the live profile and apply it. Stops a playing custom effect. The profile does not need a name. |
| `PlayCustomEffect` | `effect` | `Ok` | Play a custom effect until stopped or replaced. |
| `StopCustomEffect` | none | `Ok` | Stop the playing custom effect and re-apply the live profile. |
| `ListProfiles` | none | `Profiles` | All saved profiles. |
| `AddProfile` | `profile` | `Ok` | Save a named profile; overwrites a saved profile with the same name. Name required and non-empty. |
| `DeleteProfile` | `name` | `Ok` | Delete the saved profile called `name`. |
| `SwitchProfile` | `name` | `Ok` | Make the saved profile called `name` the live profile. |
| `CycleProfile` | none | `Ok` | Advance to the next saved profile, wrapping around. |
| `ListCustomEffects` | none | `CustomEffects` | All saved custom effects. |
| `AddCustomEffect` | `effect` | `Ok` | Save a named custom effect; overwrites one with the same name. Name required and non-empty. |
| `DeleteCustomEffect` | `name` | `Ok` | Delete the saved custom effect called `name`. |
| `Subscribe` | none | `Ok` | Push a `StateChanged` event on this connection whenever daemon state changes. |
| `Shutdown` | none | `Ok` | Ask the daemon to exit cleanly. The `Ok` is queued before exit, but clients should tolerate the connection closing without it. |

Examples:

```json
{"id": 2, "req": {"type": "SwitchProfile", "name": "gaming"}}
{"id": 3, "req": {"type": "SetProfile", "profile": {"name": null, "rgb_zones": [{"rgb": [255, 0, 0], "enabled": true}, {"rgb": [0, 255, 0], "enabled": true}, {"rgb": [0, 0, 255], "enabled": true}, {"rgb": [255, 255, 255], "enabled": true}], "effect": "Static", "direction": "Left", "speed": 1, "brightness": "Low"}}
```

## Responses

Responses are objects tagged by `"type"`.

| Type | Fields | Meaning |
| --- | --- | --- |
| `Hello` | `protocol_version`, `daemon_version` | Handshake answer. |
| `Ok` | none | Request done. |
| `State` | `state` | A `DaemonState` object. |
| `Profiles` | `profiles` | Array of `Profile`. |
| `CustomEffects` | `effects` | Array of `CustomEffect`. |
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
| `NoSuchProfile` | No saved profile or custom effect with that name. |
| `InvalidRequest` | Unparseable line, unknown request type, or a parameter out of range; `message` says which. |
| `Internal` | Anything else; `message` has details. |

## Data types

### DaemonState

```json
{
  "keyboard": {"type": "Connected"},
  "current": { Profile },
  "custom_effect_playing": "pulse",
  "profiles": [ Profile, ... ],
  "custom_effects": [ CustomEffect, ... ],
  "version": "0.21.0"
}
```

- `custom_effect_playing` is the playing custom effect's display name,
  or `null` when none plays.
- `current` is the live profile: what the keyboard shows unless a
  custom effect is playing.

### KeyboardStatus

Tagged by `"type"`:

| Type | Fields | Meaning |
| --- | --- | --- |
| `Connected` | none | Keyboard acquired; effects are applied. |
| `Searching` | none | No keyboard found yet; the daemon retries with backoff. |
| `PermissionDenied` | `message` | Keyboard present but not openable (udev rule missing). |
| `Error` | `message` | Any other device failure. |

### Profile

```json
{
  "name": "gaming",
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

- `name`: string or `null`. Required (non-empty) only for `AddProfile`.
- `rgb_zones`: exactly 4 zones, left to right. `rgb` is `[r, g, b]`,
  each 0 to 255. A disabled zone renders black.
- `direction`: `"Left"` or `"Right"`. Only meaningful for effects that
  take a direction (see the effects table); always present.
- `speed`: integer 1 to 10. Only meaningful for effects that take a
  speed; always present.
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

`mode` is `"Change"` or `"Fill"`.

`Static`, `Breath`, `Smooth` and `Wave` run on the keyboard hardware;
everything else is driven by the daemon. This does not affect the
protocol, but hardware effects survive a daemon stop.

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
C: {"id":1,"req":{"type":"Hello","protocol_version":1}}
S: {"id":1,"resp":{"type":"Hello","protocol_version":1,"daemon_version":"0.21.0"}}
C: {"id":2,"req":{"type":"Subscribe"}}
S: {"id":2,"resp":{"type":"Ok"}}
C: {"id":3,"req":{"type":"GetState"}}
S: {"id":3,"resp":{"type":"State","state":{...}}}
C: {"id":4,"req":{"type":"SwitchProfile","name":"gaming"}}
S: {"event":{"type":"StateChanged","state":{...}}}
S: {"id":4,"resp":{"type":"Ok"}}
```

Note the event can arrive before the response that caused it; match on
`id` and `resp`/`event`, never on ordering.
