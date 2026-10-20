---
title: Simple and Oneshot Services
type: concept
description: Choose a long-running or run-once service and the readiness check that tells its dependents when it is usable.
related:
  - peios/services-and-jobs/defining-a-service
  - peios/services-and-jobs/the-service-lifecycle
  - peios/services-and-jobs/triggers-and-timers
  - peios/services-and-jobs/dependencies
  - peios/services-and-jobs/supervision
---

Choose **Simple** for a program that stays running and **Oneshot** for a task
that should finish. Then check how it reports success: a Simple service
becomes ready through `Notify` or `Alive`; a Oneshot succeeds by exiting
successfully.

The default is `Type=Simple` with `Readiness=Notify`. Before starting a new
definition, make sure the program can send `READY=1` from its main process. A
program that never sends it will time out even if its process is alive. Keep
daemons in the foreground; peinit does not follow a double-forked replacement
process.

## Oneshot

| | Simple | Oneshot |
|---|---|---|
| Process lifetime | Long-running; *is* the service | Runs once, exits |
| Readiness | `Notify` (`READY=1`) or `Alive` | Successful exit — `Readiness` ignored |
| Satisfies dependents when | Active | Completed (with or without `RemainAfterExit`) |
| Successful end state | Active (until stopped) | Completed, then Inactive (unless `RemainAfterExit=1`) |
| `ExecStartPost` fires | After readiness | After successful exit only |
| `StartTimeout` covers | Startup until readiness | The whole execution |
| Watchdog and health checks | Supervised while Active | None — `WatchdogTimeout`/`HealthCheck` are inert |
| Restart on success | n/a | Never (use a timer to re-run) |

## Simple services

A **Simple** service is a long-running daemon. peinit forks, installs the
[token](~peios/services-and-jobs/identity-and-privileges), and execs the
binary. The process *is* the service: while it runs, the service is running;
when it exits, the service has stopped. This is the default and covers the
large majority of services — `registryd`, `authd`, `sshd`, application
daemons.

The interesting question for a Simple service is *when does it count as
started* — the moment its [dependents](~peios/services-and-jobs/dependencies)
are allowed to begin. That is the **readiness** model, set by the `Readiness`
field.

### Alive

| Readiness | Ready when… | Use for |
|---|---|---|
| **Notify** *(default)* | The service sends `READY=1` through [sd_notify](#the-sd-notify-contract). | Any service that has meaningful startup work — opening a socket, loading state, connecting to a backend. The dependent genuinely should wait. |
| **Alive** | The process exists — readiness is immediate. | Services with no startup handshake, where "the process is up" is as good a signal as you will get. |

The distinction matters because it is a *promise to dependents*. With
**Notify**, a service that declares itself a `Requires` target is telling
peinit "do not start anything that needs me until I say `READY=1`." With
**Alive**, peinit can only promise that the process was forked — not that it
is functional.

> [!WARNING]
> Using `Readiness=Alive` on a service that other services `Requires` is a
> common configuration smell. peinit will let it boot, but it logs a
> validation **warning**: Alive readiness gives no guarantee the service is
> actually serving before its dependents start. If a dependent connects on
> boot and gets connection-refused, this is usually why. Prefer Notify
> wherever the service can signal it.

Once ready, a Simple service transitions to **Active** and stays there until
its process exits or it is stopped. What happens on exit — clean exit, crash,
restart — is the subject of [The service
lifecycle](~peios/services-and-jobs/the-service-lifecycle) and [Keeping
services running](~peios/services-and-jobs/supervision).

## Oneshot services

A **Oneshot** service is a run-to-completion task. peinit forks, installs the
token, execs the binary, and waits for it to exit. It is the right type for
setup work: creating directories, initializing a database, running a schema
migration, applying a one-time fixup at boot.

For a Oneshot, **readiness is exit, not a signal**. The `Readiness` field is
ignored entirely — `READY=1` is meaningless for a process whose whole job is
to finish. Success and failure are decided by the exit code:

- **Exit 0** (or any code listed in `SuccessExitCodes`) → the service
  **succeeded**.
- **Any other exit, or death by signal** → the service **failed**, and the
  [failure policy](~peios/services-and-jobs/supervision) applies.

`SuccessExitCodes` is for binaries that signal a meaningful outcome with a
non-zero code — for example, a tool that exits `2` to mean "nothing to do."
Each entry is a decimal `0` –`255`; code `0` is always success and need not be
listed.

### Completed, and RemainAfterExit

A successful Oneshot transitions to **Completed**, and what happens next
depends on `RemainAfterExit`:

| `RemainAfterExit` | After a successful exit |
|---|---|
| **0** *(default)* | The service passes *through* `Completed` — releasing its dependents — then transitions to `Inactive`. |
| **1** | The service *stays* in `Completed`. |

Both forms satisfy dependents; the only difference is what a later `status`
query shows. Use `RemainAfterExit=1` when "this task is done" is a state worth
seeing — a boot-time migration you want to confirm ran, say — rather than a
service that quietly returns to `Inactive`.

### Oneshot timing and hooks

Two details distinguish a Oneshot's start sequence from a Simple one:

- **`StartTimeout` covers the entire run.** For a Simple service the timeout
  covers startup until readiness; for a Oneshot it covers everything from the
  first pre-hook to process exit. A long-running Oneshot must raise
  `StartTimeout` accordingly (or send `EXTEND_TIMEOUT_USEC` as it works — see
  [Keeping services running](~peios/services-and-jobs/supervision)).
- **`ExecStartPost` runs only on success.** Post-hooks fire after the
  successful exit. If the Oneshot fails, `ExecStartPost` does not run.

### Oneshot

A Oneshot is not restarted on *success*, regardless of `RestartPolicy` — even
`Always`. A successful exit is the goal, not a failure to retry.
`RestartPolicy` governs only the response to *failure* (a non-zero exit). To
re-run a Oneshot on a schedule, give it a [timer
trigger](~peios/services-and-jobs/triggers-and-timers); that is the intended
mechanism.

## No forking daemons

peinit does **not** support services that double-fork to background
themselves. The whole reason a traditional daemon double-forks — to detach
from the launcher — is moot when the launcher tracks the process it spawned.
peinit obtains a **pidfd** for every process at fork time, which is a kernel
handle to that exact process, immune to PID-reuse races. There is no
`MAINPID=` mechanism for a service to redirect supervision onto a different
process; peinit supervises what it forked.

If a legacy binary insists on daemonising, the fix lives at the
package build — wrap it so it stays in the
foreground (commonly a `--no-daemon` /`--foreground` flag) — not at the init
layer.

## The sd_notify contract

A Notify-readiness service signals readiness by sending `READY=1` over the
sd_notify protocol — a datagram to the socket named in the `NOTIFY_SOCKET`
environment variable, which peinit sets for *every* service. `READY=1` is only
the most visible message; the same channel carries watchdog keepalives
(`WATCHDOG=1`), graceful-stop notice (`STOPPING=1`), timeout extensions
(`EXTEND_TIMEOUT_USEC`), status strings (`STATUS=`), and the fd store. These
are covered where they belong — readiness here, the rest in [Keeping services
running](~peios/services-and-jobs/supervision) and [Controlling
services](~peios/services-and-jobs/controlling-services).

Readiness is always **per start generation**. A `READY=1` left over from a
previous incarnation of the process is never valid for the current start —
peinit ties every notify message to the specific process it is
supervising, verified by [pidfd](#no-forking-daemons).

Who is allowed to send these messages is governed by `NotifyAccess`, which
has a single value: `Main` (`0`, the default). Under `Main`, only
the tracked main PID — the process peinit forked and supervises — is
authorized to send sd_notify readiness and status messages; a message arriving
from any other PID (a child, a helper, an unrelated process) is ignored. So if
a worker your service spawns sends `READY=1`, peinit will not act on it — the
readiness signal must come from the main process itself.

## Where to start

To control *when* either type of service starts, read [Triggers and
timers](~peios/services-and-jobs/triggers-and-timers).

To follow a service from definition to running process and read its states,
read [The service lifecycle](~peios/services-and-jobs/the-service-lifecycle).

To configure restart, health checks, and watchdogs for a Simple service, read
[Keeping services running](~peios/services-and-jobs/supervision).
