# Fn+Space synchronization

Fn+Space looks like a profile switch. On these keyboards it is a race
between firmware and software.

Aurora wins that race by trusting one hardware read, counting events
and sending one final write.

## What the keyboard does

The embedded controller cycles four states:

```text
slot 1 -> slot 2 -> slot 3 -> off -> slot 1
```

It applies its own stored effect and raises a Lenovo GameZone WMI
event. Linux forwards that event through ACPI generic netlink. It does
not arrive as an evdev key.

The ITE controller also exposes a profile counter through a feature
read. That sounds like the obvious source of truth, but Aurora's own
lighting writes move the counter without raising a WMI event.

Polling therefore creates feedback:

```text
read slot -> apply profile -> write moves counter -> read false change
```

The loop can stay hidden while slots have the same color. Distinct slot
colors expose it.

## The one trusted read

Aurora reads the EC counter when it first acquires the keyboard, before
it writes lighting. It does not use later counter reads as slot
identity.

That one read is not the whole answer, though, and treating it as the
whole answer is what used to replace a user's lighting with slot 1's at
every startup. The counter and the stored selection each know something
the other does not, so the decision splits by what each source actually
knows:

| Counter says | Stored selection says | Result | Why |
| --- | --- | --- | --- |
| Off | anything | Off | Only the controller knows the backlight was turned off while the daemon was down. |
| Lit | A lit slot | The stored slot | Aurora's own writes moved the counter during the last session, so its number is not trustworthy. The stored one is. |
| Lit | Off | Slot 1 | The user turned the backlight back on while the daemon was down, and nothing recorded where they landed. |
| Unreadable, or mid-transition | anything | The stored slot | A source that cannot answer decides nothing. |

The controller decides lit versus off. The stored selection decides
which slot. Neither is trusted with the other's question.

## Count events, delay writes

The Fn+Space listener accepts both observed event encodings, matches
the Lenovo WMI device prefix and forwards every matching event to the
core module.

The core advances its logical slot immediately. It also sets a deadline
250 ms after the event. Another event advances the slot again and
replaces the deadline.

Only the final deadline writes lighting:

```text
event 1       event 2       quiet for 250 ms       apply slot 3
1 -> 2        2 -> 3        no more events          one write
```

This is not input debounce. Every event counts. Only intermediate
writes are coalesced.

That distinction matters because confirmed physical taps arrived 140
ms apart. Dropping the second event selected the wrong slot. Writing
after every event also failed during long bursts because firmware
could finish its transition after Aurora's write and leave the
keyboard dark.

## Write one complete profile

A hardware profile contains effect, speed, brightness and 12 color
bytes. Aurora replaces all fields in memory, builds one 33-byte feature
report and sends it once.

Sending separate reports for each field replays stale intermediate
state. Each report can also move the EC counter. One complete report
reduces both problems to one write.

## Selecting a slot in software

A client can select a slot without the key, through `SelectSlot`. This
moves Aurora's own slot number, which is the same number Fn+Space moves.
It cannot move the controller's counter: the hardware exposes no such
command.

The two paths therefore share one apply path, so daemon state and the
keyboard cannot disagree about which slot is live. A selection arriving
while a Fn+Space settle is still pending inherits that settle rather
than cancelling it. Applying inside the controller's own transition
window is what leaves the keyboard dark.

## State exposed to clients

The daemon reports `active_slot` as `"First"`, `"Second"`, `"Third"` or
`"Off"`. The type is closed, so an out-of-range slot cannot be
represented, let alone sent.

This is Aurora's logical state after acquisition, not a live EC counter.
Clients replace their local state with each full `StateChanged`
snapshot.

Detection can also be absent. The daemon reports `slot_sync` separately
from the slot itself, so a client can say that the key is not working on
this machine instead of implying it is. Slots still work there; they
have to be selected rather than cycled.

## Evidence

The implementation follows live tests on a 2023 Legion Pro
(`048d:c985`) and source research across Lenovo firmware, Linux WMI and
other lighting tools. See
[ITE 8295 hardware profiles](../research/ite8295-hardware-profiles.md)
for sources, traces, rejected approaches and remaining unknowns.
