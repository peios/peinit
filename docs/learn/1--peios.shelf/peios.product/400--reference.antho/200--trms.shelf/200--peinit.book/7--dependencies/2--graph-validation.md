---
title: Graph Validation
description: peinit validates the dependency graph before executing it — cycles, missing targets, and the errors and warnings it produces.
---

peinit validates the dependency graph before executing it. Validation
runs once per graph build — at boot, on an administrator-requested
on-demand start's transitive closure, and on a reload-config — and is never
incremental.
[*validate.validation-runs-once-per-graph-build]

## Cycles

peinit topologically sorts the graph; if the sort fails, a cycle exists.
Detection returns **all** cycles rather than stopping at the first: each
detected cycle's members are removed and the search re-run until the
graph is clean. [*validate.every-cycle-is-detected-not-only-the-first]

Every service in a cycle is failed with cause `CycleDetected`, and the
cycle path is logged so an administrator can see what to break.
[*validate.every-member-of-a-cycle-fails-with-cycledetected]

If any service in a cycle is Critical, peinit downgrades to Safe mode
(§2.6) rather than rebooting.
[*validate.a-critical-service-in-a-cycle-downgrades-to-safe-mode] The
cycle is a configuration error, and rebooting would find it again.

<!-- lint-disable-next-line line-length -->
## Missing targets [*validate.a-missing-hard-target-fails-the-dependent-and-a-missing-soft-one-is-dropped]

| Relationship | A missing target means |
|---|---|
| `Requires` | The dependent is failed with `DependencyFailure`. |
| `BindsTo` | The same — treated as a missing `Requires`. |
| `Wants` | Silently dropped. |
| `Conflicts` | Silently dropped. |

This holds in every mode. Safe mode drops a hard dependency on a service
it *excluded* — that is its own rule (§2.6), and without it excluding a
service would fail everything downstream and Safe mode could start
almost nothing. Only that case drops.
[*validate.safe-mode-drops-a-hard-edge-only-on-a-service-it-excluded] A
target missing from the registry or disabled by an administrator is a
configuration error rather than a Safe mode exclusion, and blocks the
dependent in Safe mode exactly as in Full.
[*validate.a-missing-or-disabled-target-blocks-in-safe-mode-too]

## Validation errors

A service that hits one is failed with cause `ValidationError` and never
started: [*validate.a-validation-error-fails-the-service-and-it-never-starts]

- The flap constraint,
  `HealthCheckRetries × HealthCheckInterval < RestartWindow` (§5.6).
- An invalid timer calendar expression (§9.1). This one is checked
  across every definition, not only those in the graph.
  [*validate.an-invalid-timer-expression-is-checked-across-every-definition]
- Two boot-triggered services that conflict with each other. Both are
  failed. Safe mode applies if either is Critical.
  [*validate.two-boot-triggered-services-that-conflict-both-fail]

## Validation warnings

Logged, and do not prevent boot:
[*validate.a-validation-warning-does-not-prevent-the-graph-from-running]

- A service using `Readiness=Alive` that something else depends on
  hard. [*validate.alive-readiness-with-a-hard-dependent-is-warned-about]
  `Alive` readiness means the process exists, which is no
  guarantee it is functional, so anything waiting on it is waiting on
  the wrong thing.
- Services that need a role no service fills — in practice `authn`,
  needed by every service whose `Identity` is not `SYSTEM`. Warned about
  rather than failed, because a missing hard dependency rejects the whole
  reload transaction, and that would prevent reloading the very
  definition that installs the missing provider. See
  [§7.6](~peios/reference/peinit/dependencies/derived-dependencies).

## Multiple findings

A service can attract more than one finding in one pass — being both in
a cycle and missing a `Requires` target, say. The runtime state records
a single primary cause, chosen by precedence:
[*validate.the-primary-cause-is-chosen-by-precedence]

1. `CycleDetected`
2. `ValidationError`
3. `DependencyFailure`

The precedence affects only which cause is stored. Every other finding
for the service is retained beside it, in discovery order, and the
operation's failure message enumerates all of them:
[*validate.every-finding-is-retained-beside-the-primary-one]

```text
CycleDetected: dependency cycle a -> b -> a (also: ValidationError:
health check interval exceeds the restart window) [2 findings]
```

The primary comes first and unqualified, so a service with one finding
reads exactly as it always did. A higher-precedence finding arriving
later demotes the previous primary rather than deleting it — breaking
the cycle should not be what it takes to discover the second fault.
[*validate.a-later-higher-precedence-finding-demotes-rather-than-deletes]

Every finding is also emitted as its own `peinit.graph.validation.failed`
KMES event, carrying `graph.phase` `boot` and the finding as
`outcome.reason`.
[*validate.each-finding-is-its-own-validation-failed-event-at-boot]
The console lines are for whoever is watching the boot; the events are
the account that survives it.

`HardDependencyBlocked` — blocked *because a dependency is blocked* —
gets its own reason, `hard-dependency-blocked`, rather than being reported as
a missing
dependency, which would claim the target does not exist when it does.
[*validate.a-blocked-dependency-is-reported-as-hard-dependency-blocked]
It has no reload-path equivalent, because reload rejects wholesale
instead of propagating a block.

The reload path behaves differently, because its consequence is
different. Validation there accumulates every finding, encodes each as
its own event under `graph.phase` `reload-config`, and then rejects the
**entire reload** — the previous generation stays live and the findings
return to the caller (§10.4).
<!-- lint-disable-next-line line-length -->
[*validate.reload-validation-rejects-the-whole-reload-under-phase-reload-config]
Boot marks individual services and continues; reload reports everything
and changes nothing.

Both use the same event type deliberately: one consumer filter catches
validation problems in either regime, and `phase` says which.

## On-demand starts

Before dispatching an administrator-requested on-demand start, peinit runs the
shared definition validator over the
services the start plan would activate. An unschedulable health check,
an invalid health-check restart window or an invalid timer schedule
rejects the plan with `INVALID_STATE`; no service from that plan is
launched. This includes definitions reached through dependencies.

Definitions outside the plan, and dependencies whose running state
already satisfies the request, do not make it fail. Missing or disabled
hard dependencies retain the ordinary blocked-plan handling; dependency
cycles retain the existing cycle refusal. Conflicting boot triggers are
not themselves a reason to reject an explicit start.

This check also applies to an administrator restart when the service has
no running process and the request builds an on-demand start plan. A queued
administrator operation is checked when promoted into such a plan; rejection
fails that operation with an `invalid_state` reason and leaves other queued
operations eligible for promotion. It does not terminate the supervisor loop.

Internal starts from restart policy, timers, BindsTo recovery, OnFailure and
TTY release retain their existing closure and cycle checks, without this
additional definition-validation refusal, including when a queued internal
operation is promoted. In particular, an invalid nonboot
OnFailure target must not roll back the source service's transition to Failed.
The stop/start leg of a restart of a running service and a deferred Backoff
restart also retain their existing internal start behavior. Boot and
reload-config validation remain as described above.
