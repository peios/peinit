---
title: Graph Execution
description: Starting everything whose dependencies are satisfied and repeating until nothing more can start — contexts, failure propagation,  and shutdown ordering.
---

Executing a graph means starting the services whose dependencies are all
satisfied, and doing it again each time something becomes satisfying,
until nothing is left.
[*exec.a-member-starts-when-every-one-of-its-dependencies-is-satisfied]

```text
execute_graph(graph, max_parallel):
    ready = services with no unsatisfied dependencies
    in_flight = 0

    while ready is not empty or in_flight > 0:
        while ready is not empty and in_flight < max_parallel:
            begin_start(ready.dequeue())
            in_flight += 1

        match wait_for_event():
            ServiceSatisfied(s):
                in_flight -= 1
                for each dependent of s:
                    if all its dependencies are satisfied:
                        ready.enqueue(dependent)

            ServiceFailed(s):
                in_flight -= 1
                propagate_failure(s)
```

`MaxParallelStarts` bounds the concurrency, counted per context as the
members running. [*exec.maxparallelstarts-is-counted-per-context]

The events are not only member completions. A dependency naming a
[readiness level](~peios/reference/peinit/dependencies/readiness-levels)
is settled against the live service table rather than the graph's own
bookkeeping, and the arrival of a `LEVEL=` — or the publisher leaving a
dependent-satisfying state — re-runs the release for every context
holding an edge on that service.

## Execution contexts

A graph execution is a retained object, not a transient loop. peinit
holds a **context** carrying its members, their dependencies and their
status, and there are two kinds: one boot context built from the boot
plan, and an on-demand context per explicit start, built from that
service's validated transitive closure.
[*exec.there-is-one-boot-context-and-one-per-explicit-start]

Both use the same scheduler, the same satisfaction rules, the same
failure propagation and the same parallelism, but they are distinct
runtime objects — which matters because they can overlap.

An operation is associated with **every** context that created or
adopted it. One operation can belong to more than one active on-demand
context: two administrators starting different services that share a
dependency both end up merged into the same already-starting operation
for it, and both contexts need to hear how it turns out.
[*exec.a-shared-dependency-is-one-operation-both-contexts-hear-about]

So when a pre-start outcome is terminal for an operation, peinit
dispatches the corresponding graph event once **per associated
context**.
[*exec.a-terminal-outcome-dispatches-one-graph-event-per-associated-context]
An operation with no associated context completes or fails
normally, and its waiters are notified, but no graph event is
dispatched.

A member whose dependent resolves without ever needing it is **pruned**
— a dormant sub-tree is canceled rather than started, so an on-demand
start that turns out not to need half its closure does not start that
half. [*exec.an-unneeded-member-is-pruned-rather-than-started]

A context is **retired** once every member has reached a terminal status
— satisfied, failed, or pruned — which is exactly when no further graph
event can be dispatched through it.
[*exec.a-context-is-retired-once-every-member-is-terminal] Its operation
associations go with it, except where an operation is also associated
with a context that is still live.

Retirement happens on the operation-maintenance sweep rather than at the
moment the last member goes terminal. The callers of a terminal outcome
walk the graph events it produced and release the contexts those events
name, so dropping a context inline would pull it out from under the rest
of the same turn's work.

Without this a context and its associations persisted for the lifetime
of the process. The memory was the smaller half: dispatching a terminal
outcome walks every context associated with the operation, so on a
long-lived machine where services are started regularly the terminal
path grew steadily slower, in PID 1's single thread.

## Failure propagation

When a service enters Failed during graph execution:

1. Everything that `Requires` or `BindsTo` it transitions to Failed with
   cause `DependencyFailure`.
2. Everything that `Wants` it is unaffected and starts normally.
3. Propagation is transitive: if A requires B and B requires C, and C
   fails, then B fails and then A fails.
   [*exec.failure-propagation-is-transitive]

## On-demand start

Starting a service explicitly resolves its dependencies first:
[*exec.an-on-demand-start-resolves-its-closure-first]

1. Collect the transitive `Requires` and `BindsTo` closure.
2. Collect the transitive `Wants` closure, best-effort.
3. Validate the sub-graph — cycles, missing targets.
4. Resolve conflicts, stopping whatever has to stop.
5. Start the sub-graph with the same parallel algorithm. Dependencies
   started this way carry cause `DependencyStart` and operation source
   `DependencyPropagation`.
   [*exec.a-dependency-started-on-demand-carries-dependencypropagation]

A service already in a satisfying state — Active, Completed, or Skipped —
is not restarted. Its dependency is already met.
[*exec.a-dependency-already-in-a-satisfying-state-is-not-restarted]

A **disabled** hard-dependency target blocks the dependent, on both
paths. [*exec.a-disabled-hard-dependency-target-blocks-the-dependent]
`Disabled` suppresses automatic activation and the service can
still be started explicitly (§3.2) — but nobody started *this* one
explicitly. Something that requires it did, and its administrator took it
out of service deliberately, so starting it to satisfy someone else's
dependency would defeat the flag by a route its description does not
consider.

Starting the disabled service itself still works. That is the escape
hatch, and it is unchanged.
[*exec.a-disabled-service-can-still-be-started-explicitly]

A disabled `Wants` target is skipped rather than blocking anything: a
soft dependency is advisory, so there is nothing to fail.
[*exec.a-disabled-wants-target-is-skipped-rather-than-blocking]

## Shutdown ordering

Shutdown reverses the graph: services with no dependents stop first,
services that everything depends on stop last, derived by reverse
topological sort from the same edges.
[*exec.shutdown-stops-dependents-before-their-dependencies]

Only hard dependencies — `Requires` and `BindsTo` — contribute to the
ordering. A `Wants` dependent can therefore be stopped after its target.
[*exec.only-hard-dependencies-order-the-shutdown]

The ordering is entirely emergent from what the definitions declare.
There is no floor and no pinning, so where the TCB services end up in
the wave order depends on their declared dependencies being right.
[*exec.there-is-no-floor-or-pinning-in-the-shutdown-order]
