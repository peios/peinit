---
title: The Command × State Matrix
description: The defined answer for every command sent to a service in an unexpected state, including backoff, abandoned and definition-removed services.
---

A command sent to a service in an unexpected state gets an answer, not a
silent no-op. What the answer is depends on the pair.

| | Inactive | Starting | Active | Reloading | Stopping | Completed | Backoff | Failed | Abandoned | Skipped |
|---|---|---|---|---|---|---|---|---|---|---|
| start [*dispatch.matrix.start] | Start | MERGE | ALREADY | ALREADY | QUEUE | Start | DEFER | Start | ERROR | Start |
| stop [*dispatch.matrix.stop] | NOOP | Cancel+Stop | Stop | Stop | MERGE | Clear | Clear | NOOP | ERROR | NOOP |
| restart [*dispatch.matrix.restart] | Start | QUEUE | Restart | Restart | QUEUE | Start | Restart | Start | ERROR | Start |
| reload [*dispatch.matrix.reload] | ERROR | ERROR | Reload | MERGE | ERROR | ERROR | ERROR | ERROR | ERROR | ERROR |
| reset [*dispatch.matrix.reset] | NOOP | ERROR | ERROR | ERROR | ERROR | ERROR | ERROR | Clear | Clear | Clear |
| status [*dispatch.matrix.status] | OK | OK | OK | OK | OK | OK | OK | OK | OK | OK |

**ALREADY** — the service is already where the command would take it and
no operation of that type is in flight. peinit returns the current
status rather than an error.

**MERGE** — an operation of that type is already running. The command
merges into it; the caller receives that operation's identifier and, if
waiting, blocks on its outcome.

**DEFER** — create a Pending start operation but do not execute it until
the existing backoff deadline expires. A deferred start already present
is merged into.

**QUEUE** — the operation is queued Pending and executes after the
current one completes.

**NOOP** — the command has no effect. peinit returns the status.

**ERROR** — the command is invalid for the state.

**Clear** — reset to Inactive.

**Cancel** — abort the current operation, then proceed.

## Backoff

Backoff is the interesting one, because the service is down with an
automatic restart already pending.

- `start` creates or merges into a **deferred** start operation and
  honors the remaining delay. It does not short-circuit the backoff.
  If the automatic restart later becomes due, it merges into the
  administrator's operation, so the identifier the caller holds is the
<!-- lint-disable-next-line words spelling -->
  one that executes. [*dispatch.backoff-start-honours-the-remaining-delay]
- `stop` cancels both the pending restart and any deferred start, and
  the service goes Inactive. A subsequent automatic restart is refused,
  because the service is not in Backoff.
  [*dispatch.backoff-stop-cancels-the-pending-restart]
- `restart` replaces the automatic restart with the administrator's.
  A Restart operation with source `admin` is queued Pending and honors
  the remaining delay exactly as a deferred `start` does; when the delay
  expires it is that operation — keeping its type, for observability —
  that executes, so the identifier the caller holds is the one that
  runs. A deferred `start` already pending is canceled with reason
  `superseded_by_restart`, and a second `restart` merges into the
  pending one rather than queueing behind it. Nothing is sent to the
  control boundary, because there is no process for it to act on.
  [*dispatch.backoff-restart-replaces-the-automatic-one]
- `reload` and `reset` are invalid: there is no process to reload, and
  no terminal state to clear.

## Skipped

`start` and `restart` clear Skipped before they run. A Skipped service
is not in a state a start can proceed from — the state machine permits
`Skipped -> Inactive` and nothing else — so the activation performs that
transition first, then re-evaluates the conditions from scratch.
[*dispatch.skipped-is-cleared-before-a-start-re-evaluates] Both
outcomes are possible: the precondition that was missing at boot can now
hold, in which case the service starts; or it can still not, in which
case the service is skipped again, for whatever reason applies now.

The clear is reported like any other transition, so a console watching
the service sees it leave Skipped rather than appearing to jump:
`peinit: service X left skipped: ExplicitStart`, before the start's own
lines. [*dispatch.the-skipped-clear-is-reported-as-a-transition]

`reset` also clears Skipped, and differs only in stopping there.
[*dispatch.reset-clears-skipped-and-stops-there]

## Abandoned [*dispatch.abandoned-accepts-only-reset]

Every lifecycle command is invalid on an Abandoned service except
`reset`, which clears it (§6.2). Nothing else is meaningful while
processes that ignored SIGKILL are still in the cgroup.

<!-- lint-disable-next-line line-length -->
## Definition-removed services [*dispatch.a-definition-removed-service-accepts-only-stop-and-status]

Independently of state, a service whose definition has been removed
(§3.8) rejects `start`, `restart` and `reload` with `UNKNOWN_SERVICE`,
accepts `stop`, and reports its state on `status`.

The accepted `stop` drains the instance with the cached `StopTimeout`,
and the exit that ends it also discards the entry. A caller waiting on
that stop is answered after the discard, with the state the service
last had — Inactive, cause `ExplicitStop` — rather than with a lookup
of an entry that has been removed. A `status` sent after that is
`UNKNOWN_SERVICE`, as for any service peinit does not know.
