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
it writes lighting. Values 1 through 3 select a remembered slot. Value
4 means off.

If the read fails or returns an unsettled value, Aurora starts from
slot 1. It does not use later counter reads as slot identity.

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

## State exposed to clients

The daemon reports `hardware_slot` as:

| Value | Meaning |
| --- | --- |
| `1`, `2`, `3` | Aurora's remembered lighting slots |
| `4` | Backlight off |
| `null` | No active keyboard slot |

This is Aurora's logical state after acquisition, not a live EC
counter. Clients replace their local state with each full
`StateChanged` snapshot.

## Evidence

The implementation follows live tests on a 2023 Legion Pro
(`048d:c985`) and source research across Lenovo firmware, Linux WMI and
other lighting tools. See
[ITE 8295 hardware profiles](../research/ite8295-hardware-profiles.md)
for sources, traces, rejected approaches and remaining unknowns.
