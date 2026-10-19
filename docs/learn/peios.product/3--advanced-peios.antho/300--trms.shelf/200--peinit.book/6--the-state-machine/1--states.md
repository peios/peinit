---
title: States
description: The ten states a service can be in, the four that satisfy dependents, and the invariants that hold across all of them.
---

Every service is in exactly one state. There are ten.
[*state.every-service-is-in-exactly-one-of-ten-states]

| State | Process? | Satisfies dependents? | Meaning |
|---|---|---|---|
| Inactive | No | No | Not started, or stopped cleanly and not set to restart. |
| Starting | Maybe | No | Activation in progress: checks, hooks, fork, or the readiness wait. The process can not exist yet. |
| Active | Yes | Yes | Running and ready. |
| Reloading | Yes | Yes | Re-reading configuration. Still satisfies dependents. |
| Stopping | Briefly | No | SIGTERM sent, awaiting exit,  or SIGKILL escalation. |
| Completed | No | Yes | Oneshot only. Exited successfully. |
| Backoff | No | No | A restart is pending; waiting out the backoff delay. |
| Failed | No | No | Exited abnormally with the restart policy exhausted or absent. |
| Abandoned | Yes, unkillably | No | SIGKILL sent and processes survived. Supervision has stopped, the cgroup is leaked. |
| Skipped | No | Yes | Conditions were not met. The service does not apply. |

## Dependent satisfaction

Exactly four states satisfy dependents: **Active**, **Reloading**,
**Completed**, and **Skipped**.
[*state.only-active-reloading-completed-and-skipped-satisfy-dependents]
Nothing else does, and a dependent blocked on a `Requires` target in any
other state does not start.
[*state.a-dependent-on-an-unsatisfied-requires-target-does-not-start]

Completed satisfies regardless of `RemainAfterExit` — a Oneshot without
it passes through Completed to release its dependents on the way to
Inactive, rather than skipping the state.
[*state.completed-satisfies-regardless-of-remainafterexit]

Skipped satisfies because a service whose conditions do not hold has
succeeded by not needing to run. Treating it as a failure would make
every conditional service a hazard to everything that depends on it.

Reloading satisfies because the process is still there and still
serving; a reload is a service telling itself to re-read a file, not an
outage. So a reload does not restart the `RestartWindow` clock (§6.4),
clear a readiness level the service has published (§7.5), or count as
its target going away to a `BindsTo` dependent (§7.1).

Backoff does not, and the distinction from Failed matters: a service in
Backoff is *going* to start again, and its dependents wait rather than
failing.
[*state.a-dependent-of-a-service-in-backoff-waits-rather-than-failing]
It is the state that makes a restart something other than a
transit through Failed. A dependent held this way looks exactly like a
start held on a readiness level — Inactive, its start operation Pending
(§8.2) — and is released when the service next reaches a
dependent-satisfying state. It fails with `dependency_failure` only when
the service gives up: its restart budget is exhausted, or it is stopped
or its definition withdrawn while in Backoff.

## Invariants

1. A service is in exactly one state at any moment. There is one state
   field and exactly one place in the implementation that assigns to it.
2. Only the transitions in §6.2 are performed. The assignment is gated
   by a central allowlist, so an unlisted transition is not merely
   avoided by convention — it cannot be written.
3. Only peinit transitions a service. The control socket produces
   operations; nothing outside peinit writes state.
   [*state.nothing-outside-peinit-writes-service-state]
4. A service object is securable independently of its process token.
   [*state.a-service-is-securable-independently-of-its-process-token] The
   ServiceSecurity descriptor governs who can manage the service; the
   process token governs what the service can reach. Neither implies
   anything about the other.
5. Readiness is per start generation. The generation increments on every
   transition into Starting,
   [*state.the-generation-increments-on-every-entry-to-starting] and a
   `READY=1` carrying a stale generation is rejected — so a notification
   from a previous incarnation can never be mistaken for this one's.
   [*state.a-ready-carrying-a-stale-generation-is-rejected]
