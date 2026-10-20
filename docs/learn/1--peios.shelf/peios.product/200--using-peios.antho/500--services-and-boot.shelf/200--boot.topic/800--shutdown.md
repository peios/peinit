---
title: Shutdown
type: how-to
description: Power off or reboot through peinit, understand graceful timeouts, and reserve forced shutdown for a stuck system.
related:
  - peios/services-and-jobs/dependencies
  - peios/services-and-jobs/supervision
  - peios/services-and-jobs/the-service-lifecycle
  - peios/boot/boot-and-boot-modes
---

Use `svctl shutdown` when you intend to stop the machine. Choose one action:

```sh
svctl shutdown poweroff
svctl shutdown reboot
svctl shutdown halt
```

These are alternatives, not a sequence. Each interrupts running services and
jobs and requires `SYSTEM_SHUTDOWN`. Save work and account for other users
before issuing one. `halt` stops the CPU but leaves power on.

A normal shutdown stops dependents before their dependencies, gives running
services time to exit, then unmounts, syncs, and performs the selected action.
Forced shutdown skips that opportunity to finish work.

## How shutdown is triggered

There are four paths into shutdown, and they are not equal — three are
graceful, one is not.

**A control command.** An administrator with `SYSTEM_SHUTDOWN` runs
`svctl shutdown <type>` (the familiar `reboot`, `poweroff` and `halt`
commands, and `shutdown -r now`, are the same client under other names):

| Type | Result |
|---|---|
| `poweroff` | Stop everything, unmount, power off. |
| `reboot` | Stop everything, unmount, reboot. |
| `halt` | Stop everything, unmount, halt (CPU stopped, power stays on). |

**Signals.** As PID 1, peinit treats a handful of signals as shutdown
requests:

| Signal | Meaning |
|---|---|
| SIGINT | Reboot — the kernel sends this on Ctrl-Alt-Del. |
| SIGTERM | Poweroff. |
| SIGPWR | Poweroff — a compatibility path for environments that surface power failure or power-button policy as a signal. |

**The power button.** On Linux, a physical power-button press is a graceful
`poweroff` — peinit watches the machine's input devices under `/dev/input` and
treats a `KEY_POWER` press (the key going down, not its release or
auto-repeat) as a shutdown request. This path is deliberately minimal and
**fail-soft**: if `/dev/input` is missing, an input device cannot be opened,
or a device stops responding, peinit keeps running and drops that device
— the button quietly stops working, but the machine stays up and you can still
shut down over the control socket or with a signal. It is not a full
power-management policy engine; a separate power-management daemon can add
richer policy on top by sending control-socket commands.

**A Critical service failure** — the ungraceful path. When an
`ErrorControl=Critical` service exhausts its [restart
budget](~peios/services-and-jobs/supervision), peinit **syncs and reboots
immediately**. There is no service-stop ordering: the system is in an
undefined state and the fastest route to a known one is a reboot. This is what
feeds the [boot-attempt
counter](~peios/boot/boot-and-boot-modes), and a Critical service
that fails identically every boot becomes a reboot loop that the counter
eventually breaks by escalating to Recovery.

## The graceful sequence

For a `poweroff`, `reboot`, or `halt`, peinit runs an ordered sequence:

1. **Enter the shutdown state.** A flag is set, and while it holds: **no new
   services can start**, timer triggers are disarmed, and new lifecycle
   commands are rejected with `INVALID_STATE`. Status, list,
   operation-status, boot, and job query/stop commands remain available so you
   can watch progress or stop a job sooner.
2. **Suspend Critical-failure semantics.** If a Critical service fails
   *during* shutdown, it is logged but does **not** trigger a reboot — the
   system is already going down, and rebooting would loop.
3. **Clear Completed services.** Oneshot services sitting in
   [Completed](~peios/services-and-jobs/the-service-lifecycle) (with
   `RemainAfterExit`) have no process; peinit moves them to Inactive so their
   dependents can be stopped cleanly.
4. **Stop every live [submitted
   job](~peios/services-and-jobs/jobs-and-operations) at once** — SIGTERM,
   then SIGKILL of the job's cgroup after its own `stop_timeout` — and cancel
   any job still queued to launch, both with cause `shutdown`. Jobs have no
   dependencies, so there is no order among them and nothing waits for them to
   finish before the service waves begin; but shutdown does not complete while
   a live job remains, within the same global timeout.
5. **Stop services in reverse dependency order**, in waves. Each
   graceful-stop-eligible service (those in Active or Reloading) gets SIGTERM
   and `StopTimeout` seconds to exit; if it does not, peinit SIGKILLs its
   whole cgroup. A service is never stopped until everything that `Requires`
   or `BindsTo` it has already stopped. Services that are Starting are not
   stopped gracefully — their startup is canceled and the cgroup SIGKILLed.
6. **Enforce the global timeout.** The whole sequence is bounded by
   `ShutdownTimeout` (default 90 s). When it expires, all remaining services
   are SIGKILLed; any whose cgroups do not empty within the post-kill timeout
   (default 5 s) become
   [Abandoned](~peios/services-and-jobs/the-service-lifecycle) (leaked), and
   shutdown continues regardless.
7. **Save the random seed.** With every service stopped and before any
   filesystem is touched, peinit writes a fresh seed drawn from the kernel's
   CSPRNG to `/var/state/peinit/random-seed` — this is the entropy the *next*
   boot starts from. The write is crash-conscious (written to a temporary
   file, flushed, then atomically swapped in), and it is best-effort: if it
   fails, peinit logs it and carries on. Shutdown never stalls on it, and the
   forced and Critical-reboot paths skip it entirely in favor of getting the
   machine down fast.
8. **Unmount filesystems.** peinit snapshots the mount table and tries to
   unmount **every** remaining non-root mount in its namespace — not only the
   ones it mounted itself — working from the deepest mount points outward.
   That includes the Phase 1 set (`/proc`, `/sys`, `/dev`, `/dev/pts`,
   `/dev/shm`, `/run`, `/sys/fs/cgroup`) wherever those are still mounted.
   A mount that is already gone counts as done. If one won't unmount because
   it is busy, peinit falls back to remounting it read-only; anything it still
   can't clean up is logged and left for the final `sync()` and the kernel,
   never blocking shutdown. The root filesystem (`/`) is never unmounted —
   once the rest are handled, peinit remounts root read-only.
9. **Sync and finish.** `sync()` to flush pending writes, then the final
   action — power off, reboot, or halt.

The reverse ordering falls straight out of the [dependency
graph](~peios/services-and-jobs/dependencies); there is no separate stop-order
configuration. Shared platform services stop after their dependents, with
`registryd` last. The exact order is derived from the installed graph.

## STOPPING=1 and timeout extension

A service that begins shutting down on its own — say, in response to an
internal error — can tell peinit by sending `STOPPING=1` through
[sd_notify](~peios/services-and-jobs/service-types). peinit acknowledges it
and, importantly, **stops sending SIGTERM** to that service: it is already on
its way down, and a redundant signal could interfere.

`STOPPING=1` is a courtesy, **not** a timeout extension. The `StopTimeout`
keeps running; if the process has not exited when it expires, peinit escalates
to SIGKILL regardless. A service that genuinely needs longer must say so with
`EXTEND_TIMEOUT_USEC` (see [Keeping services
running](~peios/services-and-jobs/supervision)) — and during shutdown those
extensions are capped not only by the per-service `StopTimeout` × 4 but also
by the remaining global `ShutdownTimeout`, whichever is smaller.

## Forced shutdown

> [!WARNING]
> Forced shutdown kills processes without waiting for graceful completion.
> Unfinished application work can be lost. Use it only when the normal
> shutdown cannot finish.

When the machine must be stopped despite a stuck graceful shutdown, **repeated
SIGINT** forces it: three Ctrl-Alt-Del presses within five seconds skip the
graceful sequence entirely — SIGKILL everything, sync, reboot. It is the
escape hatch for a graceful shutdown that is itself stuck behind an
unresponsive service.

## Shutdown during boot

A shutdown requested while peinit is still in Phase 2 boot takes effect
immediately: services still Starting are SIGKILLed (they never reached Active,
so there is nothing to stop gracefully), services that did reach Active are
stopped per the normal sequence, and the boot is abandoned.

## How peinit handles signals

peinit handles **all** signals through a `signalfd` read from its event loop —
every signal is blocked and read as data, so there are no async signal
handlers and no async-safety hazards. PID 1 cannot be killed by any signal;
the kernel protects it.

| Signal | Behavior |
|---|---|
| SIGCHLD | Reap children; match exits to services and submitted jobs. Also reaps orphaned processes the system reparented to PID 1. |
| SIGINT | Reboot request. Repeated within the window → forced reboot. |
| SIGTERM | Poweroff request. |
| SIGPWR | Poweroff request — compatibility path for power-button/power-failure policy. |
| SIGHUP | Ignored — PID 1 has no controlling terminal. |
| SIGPIPE | Ignored — a broken control-socket pipe must never crash PID 1. |

All other signals are ignored.

> [!NOTE]
> Reaping orphans matters: when any process anywhere on the system dies
> leaving children, those children are reparented to PID 1. peinit reaps them
> so they do not accumulate as zombies — even though they were never its
> services. This is part of the job of being PID 1, distinct from supervising
> services.

## Where to start

To understand the reverse ordering and which relationships gate it, read
[Dependencies and ordering](~peios/services-and-jobs/dependencies).

To understand the Critical-failure path that bypasses graceful shutdown, read
[Keeping services running](~peios/services-and-jobs/supervision).

For the boot side of the lifecycle and the reboot/recovery escalation, read
[Boot and boot modes](~peios/boot/boot-and-boot-modes).
