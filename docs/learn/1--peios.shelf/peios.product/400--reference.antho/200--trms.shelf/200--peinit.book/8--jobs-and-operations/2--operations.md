---
title: Operations
description: A requested state machine action as a first-class object — every control command creates one rather than mutating state directly.
---

An operation is a requested state machine action on a service, as a
first-class object. Control commands do not mutate state directly: every
one creates an operation that is validated, queued, resolved against
whatever else is in flight, and executed by the event loop.
[*op.every-control-command-creates-an-operation]

Operations exist because peinit serves concurrent callers —
administrative tools, automated triggers, other services. Without them,
two commands arriving together collide with whatever behavior falls
out; with them, the resolution is explicit and observable.

## Lifecycle [*op.the-operation-states]

```text
Pending --> Running --> Completed
  |           |
  +-> Merged  +-------> Failed
  |           |
  +-> Cancelled +-----> Aborted
  |
  +-> Failed
```

| State | Meaning |
|---|---|
| Pending | Validated and queued, waiting on a precondition. |
| Running | Executing. |
| Completed | The goal was reached. Start: Active for Simple, Completed,  or Inactive for Oneshot. Stop: the service is stopped. Reload: the reload resolved. |
| Failed | The goal was not reached — or the operation's maximum lifetime expired while it was still Pending (a start peinit is holding on a dependency has no lifetime, §7.5) — or peinit could not execute it (below). |
| Merged | Merged into an existing identical operation, whose identifier is recorded. |
| Canceled | Terminated while Pending. It never executed. |
| Aborted | Terminated while Running. |

An operation can also fail because peinit could not execute it: the
matrix admitted it and then the boundary found nothing to act on — no
current main job, a transition the state machine does not permit. That
is peinit's fault rather than the service's, and it is contained to the
one operation. The operation fails with the result
`internal_error: <cause>`, the service keeps the state it had, a
`[FAILED]` console line names the service, the operation and the cause
(`peinit: service <service>: <type> operation <id> failed before it
began: <cause>`), and the runtime loop carries on. A caller waiting on
the operation is answered `INTERNAL_ERROR` (§10.1).

The same rule covers every path that is about one service — a job
terminal, a setup status, a health probe, a lifecycle deadline, a notify
datagram, an event the ring refuses. An internal error there fails that
service under `InternalError` (§6.3): its job is retired, its operation
fails with `internal_error: <step>: <cause>`, a `[FAILED]` line
`peinit: service <service>: internal error at <step>: <cause>` and a
`peinit.internal-error.contained` event (§8.4) announce it, and the runtime
loop
carries on. Only an error about supervision itself — the wait, event
source registration, the listeners, the event ring refusing even the
small `peinit.event.dropped`, shutdown finalization — ends the loop and
enters recovery (§2.8). A bug peinit can attribute to one service costs
that service; it does not cost the machine its control socket.

A lifecycle deadline that raised is removed once its error is
contained, so it is announced once, not again on every timer turn.
[*op.a-deadline-that-raised-is-contained-once] One that peinit cannot
remove is retried at most once a second, and a retry that raises the
same error and changes nothing is not announced again.
[*op.a-deadline-that-keeps-raising-is-paced]

Canceled and Aborted are the same idea at different points: never ran
versus was running. Why it happened is a property of the event, not of
the state.

Two things reach Aborted. A conflicting operation superseding this one,
and a Restart whose target's definition is withdrawn while its stop leg
is draining — the stop still finishes, but there is nothing to start, so
the operation is aborted with the reason
`definition_removed_during_restart_stop_leg` (§3.8).
[*op.a-restart-is-aborted-when-its-definition-is-withdrawn-mid-stop]

A start that is aborted or canceled takes its readiness deadline with
it: the start is not waiting for `READY=1`, and ending its main
process is the superseding operation's job. A readiness deadline whose
operation has ended by the time it comes due is dropped, not acted on.
[*op.an-ended-start-leaves-no-readiness-deadline]

## Fields

```text
Operation {
    id:               GUID
    operation_type:   enum    // Start, Stop, Restart, Reload, Reset
    service:          string
    state:            enum
    created_at_ns:    u64
    started_at_ns:    u64?
    completed_at_ns:  u64?
    source:           enum
    caller:           token_summary?   // admin-initiated only
    result:           string?
    merged_into:      GUID?
}
```

## Sources

Why peinit created the operation: [*op.the-operation-sources]

| Source | Meaning |
|---|---|
| `Admin` | A control client asked for it. |
| `Boot` | The Phase 2 boot plan. |
| `Shutdown` | The shutdown lifecycle. |
| `DependencyPropagation` | A start operation created one for an unsatisfied dependency. |
| `RestartPolicy` | A restart policy generated a start. |
| `Timer` | A timer trigger fired. |
| `BindsToRecovery` | A bound target returned to Active. |
| `BindsToPropagation` | A bound target stopped. |
| `ConflictResolution` | A conflict evicted the running service. |
| `OnFailure` | A failed service's fallback handler. |
| `TtyRelease` | A service released a terminal another service was waiting for. |

Graceful shutdown creates a `Stop` operation with source `Shutdown` for
each participant in its stop waves (§12.2).
[*op.shutdown-creates-per-service-stop-operations] All are requested
when the plan is fixed; each starts when its wave becomes eligible.
These records observe the shutdown planner. They do not enter the
ordinary control queue or replace an existing administrator operation,
whose retained stopping deadline still applies. A later-wave observation
can fail its Pending lifetime without authorizing an early signal or
canceling the service's eventual stop.

A participant that exits before its wave can complete its observation
without ever starting it; a crash fails it. Once stopping, reaching
Inactive completes it, while a stop timeout, global timeout or forced
reboot fails any observation still live. A failed observation is not
completed again when the process is subsequently reaped.

## The types

**Start** creates a job for the target. Unsatisfied dependencies produce
their own start operations with source `DependencyPropagation`. It
completes when the service reaches Active, Completed or Inactive as
appropriate, or Skipped when pre-start conditions do not hold.
[*op.a-start-completes-when-the-service-reaches-its-goal-state]

**Stop** sends SIGTERM, arms `StopTimeout`, escalates to SIGKILL. It
completes when the service reaches Inactive, or Failed after a conflict
eviction or bound-dependency propagation.
[*op.a-stop-completes-when-the-service-reaches-inactive]

**Restart** is a stop then a start, tracked under one identifier across
both phases, and the type stays `Restart` throughout for observability.
[*op.a-restart-keeps-one-identifier-and-type-across-both-legs]

**Reload** issues the reload command or signal (§6.5) and completes when
the reload resolves. Unlike the other lifecycle commands it defaults to
not waiting — the caller gets the identifier immediately.

**Reset** clears Failed, Abandoned or Skipped, taking the service to
Inactive. It is synchronous. [*op.reset-is-synchronous]

## Timeouts

A start, reload, or reset inherits the target's `StartTimeout` as its
maximum lifetime; a stop inherits `StopTimeout`.
[*op.a-start-inherits-starttimeout-and-a-stop-stoptimeout] A restart has
two legs, each enforced against its own timeout, with the sum as the
overall lifetime. A start the graph is holding has no lifetime while
every unsatisfied dependency it waits on is a fact with no clock of its
own — a readiness level not yet published (§7.5), a target in Backoff
(§6.1), or a target itself held only on such facts; it stays Pending for
as long as those facts are undecided, and a caller waiting on it waits
with it. A dependency that is itself starting is bounded by that
service's `StartTimeout`, and a dependent waiting on one keeps its own
lifetime, measured from creation as below. When a hold ends, the
released start's lifetime runs from the release. Queue time behind
another operation is not a hold and still counts.
[*op.a-held-start-has-no-lifetime]

This also applies to an on-demand start whose `Requires` or `Wants` target
is already in Backoff. The request is accepted and held; it does not
bypass the target's remaining restart delay. Repeated requests merge into
the pending operations. The target's deferred start receives its lifetime
when the restart deadline releases it. If the target is stopped instead,
hard dependents fail and soft dependents can proceed.

For a transitive hold, the required release point can precede the
dependent's own dispatch: when its dependency becomes clocked, the
dependent must receive its full lifetime even if it is still Pending,
waiting for that dependency to finish. Its later dispatch must not
restart that lifetime again.

**The clock starts at creation, including queue time** — except across
a hold (§7.5), during which it does not run. From the caller's point of
view they have been waiting since they sent the command, not since
peinit got round to it. A start that sits Pending behind a stop for
longer than `StartTimeout` fails without ever running.
[*op.the-operation-clock-starts-at-creation-including-queue-time]

## Retention

Pending and Running operations are held in memory. A terminal operation
is emitted as an event and dropped after a grace period of 60 seconds —
long enough for a polling client to collect the result.
[*op.a-terminal-operation-is-dropped-after-sixty-seconds]

peinit keeps no operation history, for the same reason it keeps no job
history. eventd is the historian.
