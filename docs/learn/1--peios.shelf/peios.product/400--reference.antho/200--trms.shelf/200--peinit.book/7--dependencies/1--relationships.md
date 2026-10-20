---
title: Relationships
description: The four relationship types — Requires, Wants, BindsTo,  and Conflicts — and what each means at start, at stop and on failure.
---

Four relationship types. Each says something different about how two
services interact at start, at stop, and on failure.

## Requires

A hard dependency. If A `Requires` B:

- **Start.** B has to reach a dependent-satisfying state before A
  starts.
  If B is not running, peinit starts it with cause `DependencyStart`.
  [*rel.a-requires-target-is-started-first-with-dependencystart] If B
  enters Failed, A goes to Failed with cause `DependencyFailure`
  without attempting to start.
  [*rel.a-failed-requires-target-fails-the-dependent]
- **Stop.** Stopping B does not stop A.
  [*rel.stopping-a-requires-target-does-not-stop-the-dependent] This is
  a start-ordering constraint, not a runtime coupling.
- **Runtime failure.** If B crashes while A is Active, A is unaffected
  and keeps running.
  [*rel.a-requires-target-crashing-leaves-an-active-dependent-alone]
  B's own restart policy handles B.
- **Missing target.** A goes to Failed with `DependencyFailure`,
  detected at graph validation.
  [*rel.a-missing-requires-target-fails-the-dependent-at-validation]

## Wants

A soft dependency. If A `Wants` B, peinit starts B before A — with
cause `DependencyStart` — provided B exists and is not disabled.
[*rel.a-wants-target-that-exists-and-is-enabled-is-started-first] If B
fails to start, or does not exist at all, A starts anyway.
[*rel.a-failing-or-absent-wants-target-does-not-stop-the-dependent]
There is no stop or failure effect in either direction.

The waiting rule is where the difference from `Requires` actually lives:
a dependent blocked on a `Requires` target waits for it to reach a
*satisfying* state, while a dependent blocked on a `Wants` target waits
only for it to reach *any terminal* state, satisfying or not.
[*rel.a-wants-dependent-waits-only-for-a-terminal-state] That is
what makes `Wants` ordering rather than dependency — "start this first
if you can, but I will work without it".

## BindsTo

A runtime coupling. If A `BindsTo` B:

- **Start.** Identical to `Requires`.
  [*rel.bindsto-starts-exactly-as-requires-does]
- **Stop.** If B stops for *any* reason — explicit stop, conflict,
  crash, shutdown — A stops too, transitioning to Stopping with cause
  `BindsToPropagation`.
  [*rel.a-bindsto-target-stopping-stops-the-dependent]
- **Recovery.** When B returns to Active, peinit automatically restarts
  anything sitting in Failed with cause `BindsToPropagation` from B's
  stop. [*rel.a-bindsto-target-returning-to-active-restarts-the-dependent]
  This is reactive rather than polled: peinit watches for the
  transition into Active from a non-satisfying state, and reacts to it.
  These restarts do **not** consume the restart budget — the dependent
  never failed on its own, it was stopped because its binding target
  went away.
  [*rel.a-bindsto-recovery-restart-does-not-consume-the-restart-budget]

`BindsTo` implies `Requires`. A definition can list both for clarity,
and if it does, the `BindsTo` semantics apply.
[*rel.bindsto-implies-requires]

## Conflicts

Mutual exclusion. Starting A while B is Active creates a stop operation
for B — source `ConflictResolution`, cause `ConflictEviction` — and A
does not start until B has left every state in which it could still be
running. [*rel.starting-a-service-evicts-an-active-conflicting-one]

Conflicts are **symmetric**. If A declares `Conflicts = ["B"]`, starting
either one stops the other; B does not have to declare it reciprocally,
and peinit scans both a starting service's own conflicts and everything
that declares a conflict against it. [*rel.conflicts-are-symmetric]

If both A and B are boot-triggered and they conflict, graph validation
detects an unresolvable conflict and fails both with `ValidationError`.

A missing conflict target is silently dropped — there is nothing to
conflict with. [*rel.a-missing-conflict-target-is-dropped]

> [!NOTE]
> `Conflicts` is for real mutual exclusion: two services binding the
> same port, or two implementations of one role where exactly one must
> run. It is not a way to solve resource contention that has a better
> answer.

## Ordering and self-reference

Dependencies imply start ordering, and — except for `BindsTo` — imply
nothing about stopping. peinit starts dependencies before dependents;
during shutdown it reverses the same graph and stops dependents before
dependencies (§12.2), derived from the one graph rather than from a
separate stop-ordering configuration.

A service can not name itself in any dependency field. peinit rejects a
self-reference at graph validation as a `CycleDetected` failure, which
is what it is — a cycle of length one.
[*rel.a-self-reference-is-a-cycle-of-length-one]
