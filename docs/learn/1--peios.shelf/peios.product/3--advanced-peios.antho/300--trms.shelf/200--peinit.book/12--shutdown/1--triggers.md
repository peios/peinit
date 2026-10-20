---
title: Triggers
description: The four paths that initiate a shutdown — the control socket, signals, the power button,  and a Critical service failure.
---

Four paths initiate a shutdown.

## The control socket

A `shutdown` command naming a type, gated on `SYSTEM_SHUTDOWN` against
peinit's control descriptor (§4.7):
[*sdtrig.a-shutdown-command-names-poweroff-reboot-or-halt]

| Type | Effect |
|---|---|
| `poweroff` | Stop everything, unmount, power off. |
| `reboot` | Stop everything, unmount, reboot. |
| `halt` | Stop everything, unmount, halt — the CPU stops, the system stays powered. |

## Signals [*sdtrig.sigint-reboots-and-sigterm-and-sigpwr-power-off]

| Signal | Meaning |
|---|---|
| SIGINT | Reboot. The kernel sends it on Ctrl+Alt+Del. |
| SIGTERM | Poweroff. PID 1 cannot be killed by it but can choose to act on it. |
| SIGPWR | Poweroff. The compatibility path for environments that surface power failure or a power-button policy as a signal. |

**Three SIGINTs within five seconds force an immediate shutdown**: no
graceful stop, no ordering, SIGKILL every service cgroup, sync, reboot.
[*sdtrig.three-sigints-in-five-seconds-force-an-immediate-reboot]
The window is a sliding five seconds and the press is recorded before
the already-shutting-down check, so three presses still force even after
a graceful reboot has begun.
[*sdtrig.three-presses-force-even-after-a-graceful-shutdown-has-begun]
That is the point — someone pressing it
three times has decided the graceful path is not working.

## The power button

An `EV_KEY` / `KEY_POWER` press from a readable `/dev/input/event*`
device is a graceful `poweroff`.
[*sdtrig.a-power-button-press-is-a-graceful-poweroff] Only a press —
value 1 — initiates. Releases, key repeats, other keys, and other event
types are ignored. [*sdtrig.only-a-key-press-initiates]

The path is fail-soft throughout: a missing `/dev/input`, a device that
cannot be opened or registered, and a registered descriptor that later
fails to read are all survivable, and a failing descriptor is removed
from the event loop so repeated failures cannot spin PID 1.
[*sdtrig.the-power-button-path-is-fail-soft] Losing it
degrades only direct power-button handling; the socket and signal paths
remain.

It is deliberately minimal. It is not a power-management policy engine
and does not replace a future daemon that would translate richer policy
into control socket commands.

## Critical service failure

A Critical service entering Failed with its restart budget exhausted
means peinit syncs the filesystems and reboots immediately.
[*sdtrig.a-critical-service-out-of-restart-budget-reboots-immediately]
This is not
a graceful shutdown: there is no stop ordering, no seed save, and no
unmount. The system is in an undefined state and the fastest path to a
defined one is a reboot.
