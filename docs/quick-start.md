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
daemon:   running (v0.24.0)
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

These are the defaults a new profile starts with. Existing settings keep
their saved slot colors.

Check the active slot at any time:

```console
$ nix run github:HughScott2002/Aurora-Legion#daemon -- status
slot:     2 of 3 (Static effect)
```

You can also move between slots without the key:

```console
$ nix run github:HughScott2002/Aurora-Legion#daemon -- slot 3
slot 3 selected
```

## 3. Change the active slot

Set the active lit slot to magenta:

```console
$ nix run github:HughScott2002/Aurora-Legion#daemon -- set \
    -e Static -c 255,0,255,255,0,255,255,0,255,255,0,255
lighting applied
```

Cycle through Fn+Space again. Magenta returns when you reach that slot.
Aurora saved the change in the daemon settings.

The other two slots kept their own colors. All three belong to the same
profile, so saving that profile saves all three together.

## 4. Open and close the GUI

Run the native app:

```console
$ nix run github:HughScott2002/Aurora-Legion
```

The slot buttons at the top of the Lighting page do what Fn+Space does.
Everything below them edits the slot you have selected.

Change an effect, then close the window. The lighting stays because the
daemon owns it.

## Next

- [Install Aurora](README.md#how-to-guides) for regular use.
- [Learn the CLI](reference/cli.md).
- [Understand the architecture](explanation/architecture.md).
