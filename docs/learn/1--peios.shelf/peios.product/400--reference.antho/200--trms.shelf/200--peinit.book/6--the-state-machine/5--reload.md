---
title: Reload
description: Telling a service to re-read its configuration without restarting — choosing the signal or command path, auto-detection, and interruptions.
---

Reload tells a service to re-read its configuration without restarting.
peinit issues the reload, moves the service to Reloading, and resolves
it one of three ways. [*reload.a-reload-moves-the-service-to-reloading]

A failed reload never takes a running service out of Active.
[*reload.a-failed-reload-never-takes-a-service-out-of-active] And reload
never gets stuck: every path has a timeout.
[*reload.every-reload-path-has-a-timeout]

## Choosing a path

`ExecReload` absent means SIGHUP to the main process.
[*reload.an-absent-execreload-means-sighup] A `signal:<NAME>`
value means that signal instead.
[*reload.a-signal-value-sends-that-signal] Anything else is a command,
forked into the service's `hooks/` sub-cgroup under the service's
**own** identity — never peinit's token, and `HookIdentity` does not
apply.
[*reload.a-reload-command-runs-in-hooks-under-the-services-own-identity]

## The signal path

There is no command exit to observe, so completion is inferred from the
main process's own notifications.
[*reload.the-signal-path-resolves-on-the-main-processs-notifications]

```text
start the detection window (2 seconds)

on READY=1, at any time:
    -> Active, "confirmed"

on RELOADING=1 within the window:
    cancel the window
    start the extended wait (StartTimeout)

on the window expiring with no RELOADING=1:
    -> Active, "advisory"

on the extended wait expiring with no READY=1:
    -> Active, "advisory"
```

The two-second window is a constant and is not configurable through the
registry, directly or otherwise.
[*reload.the-detection-window-is-a-fixed-two-seconds] It is not clamped
to the operation's own deadline: a service configured with a short
`StartTimeout`, which is a reasonable thing to do for something that
starts fast, still gets its full two seconds to answer a reload.
[*reload.a-short-starttimeout-does-not-shorten-the-detection-window] The
reload operation's own lifetime already bounds the whole thing.

The extended-wait expiry means the service announced a reload and never
finished one. That is what the detection protocol exists to catch — the
service has wedged mid-reload or lost its handler — so it is reported
rather than only returned:
[*reload.an-unconfirmed-reload-is-reported-on-the-console-and-audited]

```text
peinit: service <service> signalled RELOADING=1 but never completed reload
```

and audited as a `peinit.service.reload.timed-out` event. The outcome is
still carried in the operation's result for a `wait=true` caller,
[*reload.the-outcome-is-carried-in-the-operations-result] but a
reload issued without waiting reports its outcome on the console as well.

A detection window expiring is a different thing and stays quiet.
[*reload.a-detection-window-expiring-stays-quiet] A
service that does not implement the handshake lets it expire on every
reload, and reporting that would drown the case above.

## The command path

The command's exit gates failure; the main process's `READY=1` gates
confirmation.
[*reload.the-command-exit-gates-failure-and-ready-gates-confirmation]

```text
on the command exiting non-zero:
    -> Active, "failed"

on the command exceeding StartTimeout:
    SIGKILL the hooks/ sub-cgroup, taking its descendants
    -> Active, "failed"

on the command exiting zero:
    -> Active, "confirmed" if the main process sent READY=1 during the
       reload, otherwise "advisory"
```

## Auto-detection

The protocol needs no per-service configuration. A service that
implements the notification handshake — `RELOADING=1` then `READY=1` —
gets real lifecycle tracking. One that does not gets a brief Reloading
state that resolves itself when the detection window expires. Neither
has to declare which it is.
[*reload.the-handshake-needs-no-per-service-configuration]

## Interruptions

**The main process crashes while Reloading.** That is a `ProcessCrash`,
and the restart policy is consulted: Reloading to Backoff if a restart
is allowed and the budget holds, Reloading to Failed otherwise.
[*reload.a-crash-while-reloading-is-a-processcrash-on-the-restart-path]
Both reload timers are canceled and any in-flight reload command is
killed. [*reload.a-crash-cancels-the-timers-and-kills-the-reload-command]

This is a different event from an external reload *command* exiting
non-zero, which is the "failed" outcome above and leaves the main
process — and the service — running.

**A stop arrives while Reloading.** peinit cancels the reload
immediately: it drops the reload deadlines, kills any reload command's
cgroup, and sends SIGTERM in the same turn, without waiting out the
window or the extended wait. The service goes to Stopping and the reload
operation is aborted.
[*reload.a-stop-while-reloading-cancels-the-reload-and-sigterms-at-once]
