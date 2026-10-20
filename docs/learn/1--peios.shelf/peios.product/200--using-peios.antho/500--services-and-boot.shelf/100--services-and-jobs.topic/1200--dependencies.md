---
title: Dependencies and Ordering
type: concept
description: Choose startup dependencies and runtime coupling, check why a dependent is blocked, and predict the effect of stopping a target.
related:
  - peios/services-and-jobs/the-service-lifecycle
  - peios/services-and-jobs/supervision
  - peios/boot/boot-and-boot-modes
  - peios/boot/shutdown
  - peios/services-and-jobs/jobs-and-operations
---

Check a service's dependencies before stopping a shared component or
diagnosing a blocked start. **Requires** waits for successful startup but
leaves an already-running dependent alone if the target later stops.
**BindsTo** also stops the dependent when its target goes away.

Relationships are set on the service that needs something, naming the target
it needs. **Wants** attempts an optional target first; **Conflicts** makes two
services mutually exclusive and is symmetric, so one side declaring it is
enough.

## Before changing a dependency

1. Read the dependent's definition with `svctl definition show <service>` and
   inspect each target with `svctl status <target>`.
2. Decide whether the requirement is successful startup (`Requires`), optional
   startup ordering (`Wants`), runtime coupling (`BindsTo`), or mutual
   exclusion (`Conflicts`).
3. Use a [readiness level](#waiting-for-a-condition-not-only-a-service) when
   the target must have reached a specific condition, such as a routed
   network.
4. After saving, validate the definition and check the next start. Dependency
   edits apply on a new start or graph reload; they do not rewrite a running
   process's history.

A `DependencyFailure` usually points to the target to investigate first. An
on-demand start can pull in other services and evict conflicting ones, so its
effects can extend beyond the service you named.

## The four relationships

### Requires — a hard dependency

If A `Requires` B:

- **Start.** B must reach a [satisfying
  state](~peios/services-and-jobs/the-service-lifecycle) before A starts. If B
  is not running, peinit starts it first (cause `DependencyStart`). If B
  fails, A is marked Failed with `DependencyFailure` and never even attempts
  to start.
- **Stop.** Stopping B does **not** stop A. Requires is a *start-ordering*
  constraint, not a runtime leash — A keeps running.
- **Runtime failure.** If B crashes while A is already Active, A is
  unaffected. B's own [restart policy](~peios/services-and-jobs/supervision)
  handles B's recovery.

Requires is the workhorse: "I need this to have started, and if it can't,
neither can I."

### Wants — a soft dependency

If A `Wants` B:

- **Start.** peinit starts B before A *if* B exists and is not
  [Disabled](~peios/services-and-jobs/triggers-and-timers) — but if B fails to
  start, or does not exist, **A starts anyway.**
- **Stop / failure.** No effect either way. A and B are runtime-independent.

Wants is best-effort ordering: "start this first if you can, but I work
without it."

### BindsTo — a runtime coupling

If A `BindsTo` B:

- **Start.** Identical to Requires — B must satisfy before A starts.
- **Stop.** If B stops for *any* reason — explicit stop, conflict, crash,
  shutdown — A is stopped too, transitioning to Stopping with cause
  `BindsToPropagation`. When A finishes stopping it comes to rest in
  **Failed** (carrying `BindsToPropagation`), *not* Inactive — so a `status`
  query shows the bound service as Failed, making it clear it was taken down
  by its target rather than shut down cleanly.
- **Recovery.** When B returns to Active, peinit **automatically restarts** A
  from that Failed state (cause `BindsToRecovery`). This is reactive — peinit
  watches B's transitions — and it is **budget-exempt**: A did not fail on its
  own, so the restart does not count against its
  [budget](~peios/services-and-jobs/supervision). If B never comes back, A
  stays Failed until you `reset` (or start) it.

BindsTo is Requires plus a runtime leash: "I need this to start, *and* I
should not outlive it." It is the right choice for a sidecar that is
meaningless without its principal. `BindsTo` implies `Requires`; listing both
for the same target is harmless, and BindsTo semantics win.

### Conflicts — mutual exclusion

If A `Conflicts` with B:

- **Start.** Starting A while B is Active creates a stop operation for B
  (source `ConflictResolution`), evicting it (cause `ConflictEviction`)
  before A starts — and vice versa. If the loser will not stop within its
  `StopTimeout`, [SIGKILL escalation](~peios/boot/shutdown)
  applies. The evicted loser does **not** land in Inactive: it comes to rest
  in **Failed** (carrying `ConflictEviction`), so it shows as Failed in
  `status` and needs a `reset` before it will start again. That is deliberate
  — an evicted service is not a clean stop, and leaving it Failed stops it
  from quietly restarting straight back into the conflict.
- **Symmetry.** Conflicts is two-way. If A declares `Conflicts=["B"]`,
  starting *either* stops the other; B need not declare it back.

Conflicts is for true mutual exclusion — two services binding the same port,
or two implementations of one role where exactly one must run — not for
ordinary resource contention.

### At a glance

| | Start order | Target failure stops dependent? | Target stop stops dependent? |
|---|---|---|---|
| **Requires** | target first; dependent fails if target fails | At start time only | No |
| **Wants** | target first if present; dependent starts regardless | No | No |
| **BindsTo** | target first; dependent fails if target fails | Yes (and auto-recovers) | Yes (and auto-recovers) |
| **Conflicts** | starting one evicts the other | n/a | n/a |

### Waiting for a condition, not only a service

A target can carry a **readiness level** after a colon.
`Requires = ["network:routed"]` waits not merely for the network manager to be
active but for it to have published the level `routed` — a default route in
place — and holds the start until it does. Publishers report a set of
true levels, and peinit checks membership. A routed network
publishes `link`, `addressed` and `routed`, so a service can ask for the
condition it actually needs. peinit clears the set the moment its publisher
leaves a satisfying state.

`network` there is a **role**, not a service: the service that fills it
declares `Provides = ["network"]`, and peinit rewrites the entry to that
service before anything else looks at it. Write the role, not the daemon. The
levels `link`, `addressed` and `routed` are defined by network
policy, so the definition keeps meaning the same
thing whichever executor an image ships. `svctl status` shows the resolved
dependency, `netd:routed` on a standard image.

## On-demand starts

When you start a service explicitly rather than at boot, peinit does the same
graph work on a smaller scope — the requested service's transitive closure:

1. Collect all transitive `Requires` and `BindsTo` dependencies (and
   best-effort `Wants`).
2. Validate that sub-graph (cycles, missing targets).
3. Resolve `Conflicts` — stop anything that conflicts.
4. Start the sub-graph with the same parallel scheduler.

Anything already in a satisfying state (Active, Reloading, Completed, Skipped)
is left alone — its dependency is already met, so there is no needless
restart. Dependencies pulled in this way start with cause `DependencyStart`.
If two on-demand starts need the same dependency at once, their start
operations [merge](~peios/services-and-jobs/jobs-and-operations) rather than
racing.

## Failure propagation

When a service enters Failed during graph execution, the failure spreads along
`Requires` and `BindsTo` edges — and only those:

1. Every service that `Requires` the failed one transitions to Failed with
   `DependencyFailure`.
2. Every service that merely `Wants` it is **unaffected** and starts normally.
3. Propagation is **transitive**: if A requires B and B requires C, and C
   fails, then B fails, then A fails — each with `DependencyFailure`.

This is the payoff of the Requires/Wants distinction. A hard dependency
failing takes its dependents down with it; a soft one failing is shrugged off.
Choosing the right relationship is choosing how far a failure is allowed to
travel.

## Graph validation

Before peinit starts *anything*, it builds the dependency graph and validates
it. Validation runs once per graph build — at boot for the whole boot graph,
and per request for an [on-demand start](#on-demand-starts)'s transitive
closure. It is not incremental.

**Cycles.** peinit topologically sorts the graph; if the sort fails, there is
a cycle. Every service in the cycle is marked Failed with `CycleDetected`,
and peinit logs the full path (`dependency cycle: A → B → C → A`) so you can
find and break it. A service can not depend on itself — a self-reference is
rejected as a cycle. If any service in the cycle is `ErrorControl=Critical`,
peinit downgrades to [Safe mode](~peios/boot/boot-and-boot-modes)
rather than rebooting, because a reboot would hit the same cycle.

**Missing targets.** What a missing target means depends on the relationship:

| Relationship | Missing or disabled target |
|---|---|
| `Requires` | Dependent marked Failed (`DependencyFailure`). |
| `BindsTo` | Treated as a missing Requires — Failed (`DependencyFailure`). |
| `Wants` | Silently ignored — the entry is dropped. |
| `Conflicts` | Silently ignored — nothing to conflict with. |

**Unresolvable conflicts.** If two *boot-triggered* services conflict with
each other, there is no way to honor both, so both are marked Failed with
`ValidationError`. As with cycles, if either is Critical, Safe mode applies.

**Warnings.** Some conditions are logged but do not block boot — most notably
a `Readiness=Alive` service that others `Requires` (see [service
types](~peios/services-and-jobs/service-types)). Warnings appear in the logs
and do not change state.

**Validation errors.** Some conditions *do* fail the service — for example a
health-check configuration that violates the [flap
constraint](~peios/services-and-jobs/supervision). These mark the service
Failed with `ValidationError`.

When more than one finding applies to the same service, peinit records a
single primary cause by precedence — `CycleDetected` > `ValidationError` >
`DependencyFailure` — but it logs *all* findings, so the primary cause never
hides the others.

## Parallel start

After validation, peinit starts services whose dependencies are all satisfied,
and it does so **in parallel** up to a configurable limit:

| Registry key | Default | Meaning |
|---|---|---|
| `Machine\System\Boot\MaxParallelStarts` | 10 | Maximum services starting concurrently. |

The scheduler is simple: every service with no unsatisfied dependencies is
eligible; peinit starts up to `MaxParallelStarts` of them; as each one reaches
a [satisfying state](~peios/services-and-jobs/the-service-lifecycle) its
dependents become eligible and join the queue. The result is that independent
subtrees of the graph come up at the same time, while ordering constraints are
still honored exactly.

> [!NOTE]
> Boot order is *emergent*, not hardcoded. The installed definitions and their
> identity dependencies determine the sequence. Change the dependencies and
> you get a different order. The platform daemons come up first because
> everything else, directly or transitively, requires them.

## Shutdown reverses the graph

peinit does not need a separate stop-ordering configuration.
[Shutdown](~peios/boot/shutdown) **reverses** the
dependency graph: services with no dependents stop first, and services that
others depend on stop last. A service is never stopped until everything that
`Requires` or `BindsTo` it has already stopped.

Shared platform services stop after their dependents, with `registryd` last.
The exact order comes from the installed graph; do not rely on a fixed list of
daemon names. The full shutdown sequence is in
[Shutdown](~peios/boot/shutdown).

## Where to start

To see the states services move through as they start and stop in order, read
[The service lifecycle](~peios/services-and-jobs/the-service-lifecycle).

To understand how a target's failure interacts with the dependent's own
restart policy, read [Keeping services
running](~peios/services-and-jobs/supervision).

To follow how dependency-driven starts appear as operations, read [Jobs and
operations](~peios/services-and-jobs/jobs-and-operations).
