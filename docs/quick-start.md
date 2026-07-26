# Quick start

This tutorial starts Aurora, changes the keyboard and tests its
Fn+Space slots. It uses Nix without installing anything.

You need a [supported 4-zone keyboard](../driver/src/lib.rs) and
[keyboard access](how-to/install-linux.md#grant-keyboard-access).

## 1. Start the daemon

Run the daemon in one terminal:

```console
$ nix run github:HughScott2002/Aurora-Legion#daemon
```

Leave it running. Its log should report that the keyboard connected.

In another terminal, check the state:

```console
$ nix run github:HughScott2002/Aurora-Legion#daemon -- status
daemon:   running (v0.22.0)
keyboard: connected
```

If the keyboard is missing, stop here and
[troubleshoot the connection](how-to/troubleshoot.md#keyboard-not-found-or-permission-denied).

## 2. Test the slots

Press Fn+Space once, then wait a moment. Repeat until you see the full
cycle:

1. Red
2. Green
3. Blue
4. Off

These are Aurora's defaults. Existing settings keep their saved slot
colors.

Check the active slot at any time:

```console
$ nix run github:HughScott2002/Aurora-Legion#daemon -- status
hw slot:  2 of 3
```

## 3. Change the active slot

Set the active lit slot to magenta:

```console
$ nix run github:HughScott2002/Aurora-Legion#daemon -- set \
    -e Static -c 255,0,255,255,0,255,255,0,255,255,0,255
profile applied
```

Cycle through Fn+Space again. Magenta returns when you reach that slot.
Aurora saved the change in the daemon settings.

## 4. Open and close the GUI

Run the native app:

```console
$ nix run github:HughScott2002/Aurora-Legion
```

Change an effect, then close the window. The lighting stays because the
daemon owns it.

## Next

- [Install Aurora](README.md#how-to-guides) for regular use.
- [Learn the CLI](reference/cli.md).
- [Understand the architecture](explanation/architecture.md).
