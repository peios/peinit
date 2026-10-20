---
title: Conflict Resolution
description: Resolving a new operation against one already pending or running — merging, cross-type conflicts, and the principles behind both.
---

When a new operation is requested and one for the same service is
already Pending or Running, peinit resolves the two.

## Merging

An operation of the same type merges. The new caller receives the
**existing** operation's identifier, and from their point of view their
request is in progress — they neither know nor need to know that it
merged. [*conflict.an-operation-of-the-same-type-merges]

| Existing | New | Resolution |
|---|---|---|
| Start | Start | Merge |
| Stop | Stop | Merge |
| Reload | Reload | Merge |
| Restart | Start | Merge — a restart already includes a start |

Restart is not mergeable with itself. A second restart while one is in
progress is queued. [*conflict.restart-is-not-mergeable-with-itself]
The one place that does not apply is a restart deferred by a backoff
(§10.3): it is Pending with nothing running for a second one to queue
behind, so a second `restart` in Backoff merges into it.

## Cross-type [*conflict.the-cross-type-resolutions]

| Existing | New | Resolution |
|---|---|---|
| Start (Pending) | Stop | Cancel the start, create the stop |
| Start (Running) | Stop | Abort the start, create the stop |
| Start (Pending) | Restart | Cancel the start, queue the restart |
| Start (Running) | Restart | Queue the restart |
| Stop (either) | Start | Queue the start |
| Stop (either) | Restart | Queue the restart |
| Restart (Pending) | Stop | Cancel the restart, create the stop |
| Restart (Running) | Stop | Abort the restart, create the stop |
| Restart (either) | Restart | Queue |
| Reload (Pending) | Stop | Cancel the reload, create the stop |
| Reload (Running) | Stop | Abort the reload, create the stop |
| Reload (Pending) | Restart | Cancel the reload, create the restart |
| Reload (Running) | Restart | Abort the reload, create the restart |
| Anything (either) | Reset | Reject |

Combinations outside this table are rejected: a new Reload while a
Start, Stop or Restart is active.
[*conflict.combinations-outside-the-table-are-rejected] A new Start
while a Reload is active is not rejected — a reloading service is
already where a start would take it, so the command is answered with
the service's status and creates no operation, before conflict
resolution is reached at all.
[*conflict.a-start-while-reloading-is-answered-with-the-status]

## The principles

1. **Stop wins over start.** An explicit stop always takes priority. The
   administrator said stop, so stop; a queued start can follow.
   [*conflict.stop-wins-over-start]
2. **Later supersedes earlier.** Start then immediately stop means the
   stop wins and the start is canceled, recorded as superseded.
   [*conflict.a-superseded-start-records-that-it-was-superseded]
3. **Merging is transparent.** The merged caller gets the original
   identifier and blocks on the original operation's outcome.

Reset is rejected outright while anything is in flight, because reset
means "clear a terminal state" and nothing in flight has one.
[*conflict.reset-is-rejected-while-anything-is-in-flight]

## Dependency propagation

When a start executes against a service with unsatisfied dependencies,
peinit creates start operations for them:

- **`Requires`** — source `DependencyPropagation`. If one fails, the
  parent operation fails with `DependencyFailure`.
  [*conflict.a-failed-requires-dependency-fails-the-parent]
- **`Wants`** — source `DependencyPropagation`. If one fails, the parent
  continues. [*conflict.a-failed-wants-dependency-does-not-fail-the-parent]
- **`BindsTo` recovery** — source `BindsToRecovery`, created when a
  bound target returns to Active, for dependents sitting in Failed with
  cause `BindsToPropagation`.

Dependency-created operations follow the same resolution rules as
administrator-created ones. If a dependency is already starting because
something else also depends on it, the operations merge — and both
graph execution contexts are then associated with the one operation
(§7.3).

`BindsToRecovery` restarts are not subject to the restart budget. They
are created because a dependency returned, not because anything failed.
[*conflict.bindstorecovery-restarts-are-outside-the-restart-budget]

## Restart policy and timers

A restart-eligible failure creates a start operation with source
`RestartPolicy` once the backoff delay elapses.
<!-- lint-disable-next-line line-length -->
[*conflict.a-restart-eligible-failure-creates-a-start-with-source-restartpolicy]
It goes through the ordinary validation and resolution: if an
administrator has already sent a stop, or the budget is exhausted, it is
rejected.

A timer firing creates an operation based on the service's current
state: [*conflict.a-timer-firing-creates-an-operation-from-the-current-state]

| Type | State | Action |
|---|---|---|
| Oneshot | Inactive, Completed, Failed | Create a start, source `Timer`. |
| Oneshot | Active, Starting | Set the pending flag. One catch-up run, no operation. |
| Simple | Inactive, Failed | Create a start, source `Timer`. |
| Simple | Active, Starting | No-op. The firing is recorded. |

The Oneshot catch-up creates its start when the current run completes.
Multiple missed firings collapse into one pending run.

## Boot and shutdown are not operations

Boot and shutdown are modes peinit enters, which then generate
per-service operations. There is no "shutdown operation" to observe or
cancel. Boot-generated starts use source `Boot`.
[*conflict.boot-generated-starts-use-the-boot-source]

Shutdown-generated stops use source `Shutdown`, and they are the one
exception to the tables above. They are created outside conflict
resolution: a `Shutdown` Stop does not merge into, cancel or abort an
administrator's Stop or Restart already in flight, but exists beside it,
so that operation keeps its identity and its retained timing (§12.2).
