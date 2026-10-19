---
title: Transition Causes
description: Every transition carries a cause — the taxonomy, which causes are restart-eligible, OnFailure, the loop guard and the logging contract.
---

Every transition carries a cause recording why it happened. peinit
tracks both the current state and the cause of the most recent
transition, and the cause determines restart eligibility, `OnFailure`
behavior, and what an administrator is told.
[*cause.every-transition-carries-the-cause-of-the-most-recent-transition]

## The taxonomy

| Cause | Leads to | Meaning |
|---|---|---|
| `ExplicitStart` | Starting | An administrator, an `OnFailure` handler, or a boot plan started it. |
| `ExplicitStart` | Inactive | An explicit start on a Skipped service, clearing it so the conditions are re-evaluated. |
| `DependencyStart` | Starting | Started to satisfy another service's dependency. |
| `RestartPolicy` | Starting | An automatic restart after a backoff delay. |
| `BindsToRecovery` | Starting | A bound dependency returned to Active. |
| `Timer` | Starting | A timer trigger fired. |
| `ExplicitStop` | Stopping | An administrator requested a stop. |
| `ExplicitReload` | Reloading, Active | A reload was issued and resolved. |
| `ExplicitReset` | Inactive | An administrator cleared Failed, Abandoned,  or Skipped. |
| `ConflictEviction` | Stopping | A conflicting service started; this one lost. |
| `BindsToPropagation` | Stopping | A bound dependency stopped. |
| `ShutdownWave` | Stopping, Failed | The system is shutting down. Active and Reloading services go to Stopping; Starting services go straight to Failed. |
| `ProcessCrash` | Failed, Backoff | The main process exited unexpectedly. |
| `CleanExit` | Inactive | A Simple process exited successfully with a policy other than Always. |
| `CleanExitRestart` | Backoff | A Simple process exited successfully under `RestartPolicy=Always`. |
| `ReadinessTimeout` | Failed, Backoff | `StartTimeout` expired before readiness. |
| `WatchdogTimeout` | Failed, Backoff | A keepalive did not arrive in time. |
| `HealthCheckFailure` | Failed, Backoff | `HealthCheckRetries` consecutive failures. |
| `PreHookFailure` | Failed, Backoff | An `ExecStartPre` hook exited non-zero, or its token failed, or the start timed out during hooks. |
| `ParentSetupFailure` | Failed, Backoff | A parent-side failure before the fork. No child was created. |
| `PreExecFailure` | Failed, Backoff | Post-fork setup failed before exec, reported through the error pipe. |
| `DependencyFailure` | Failed | A `Requires` dependency entered Failed. |
| `RestartBudgetExhausted` | Failed | `RestartMaxRetries` reached. |
| `CycleDetected` | Failed | The service is part of a dependency cycle. |
| `ValidationError` | Failed | The definition failed graph validation. |
| `AssertionError` | Failed | A start-time assert failed. |
| `ConditionSkipped` | Skipped | A start-time condition failed. |
| `TtyUnavailable` | Skipped | Another service was holding the terminal this one names in `TTYPath`. §11.6 |
| `ProcessUnkillable` | Abandoned | Processes survived SIGKILL. |
| `InternalError` | Failed | peinit could not carry out a step on the service — a relaunch, a job terminal, a setup status, a health probe, a deadline action. The service did nothing wrong; its operation fails with the `internal_error` result (§8.2). |

## Restart eligibility

Causes fall into four classes, and the class decides whether the restart
policy is consulted at all.

**Restart-eligible.** peinit consults `RestartPolicy` and the budget. If
a restart is allowed and the budget holds, the service goes to Backoff
and then to Starting; otherwise Failed.
[*cause.a-restart-eligible-cause-consults-the-policy-and-the-budget]

`ProcessCrash`, `WatchdogTimeout`, `HealthCheckFailure`,
`ReadinessTimeout`, `PreHookFailure`, `PreExecFailure`,
`ParentSetupFailure`.

> [!NOTE]
> The startup failures are restart-eligible because a transient problem
> during startup should get another go. A pre-hook that failed because a
> network mount was briefly unavailable deserves a retry rather than an
> immediate give-up.

**Always-only.** `CleanExitRestart` applies when a Simple service exits
successfully and the policy is Always. It uses the same backoff and the
same budget as a failure,
[*cause.cleanexitrestart-uses-the-same-backoff-and-budget-as-a-failure]
which is what stops a daemon that exits
cleanly in a tight loop from bypassing throttling entirely. It is never
treated as a `ProcessCrash`, and both status and events make clear the
process succeeded and was restarted only because the policy says so.

`CleanExit` is its non-restarting counterpart: straight to Inactive,
consulting neither policy nor budget.

**Budget-exempt.** `BindsToRecovery` takes a service from Failed to
Starting when its binding target returns. It is not subject to the
policy or the budget,
[*cause.bindstorecovery-is-exempt-from-the-policy-and-the-budget]
because the service did not fail on its own — it
was stopped because its dependency went away.

**Never-restart.** peinit does not consult the policy at all:
[*cause.a-never-restart-cause-does-not-consult-the-policy]
`ExplicitStop`, `ExplicitReset`, `ShutdownWave`, `ConflictEviction`,
`BindsToPropagation`, `ProcessUnkillable`, `RestartBudgetExhausted`,
`ValidationError`, `CycleDetected`, `DependencyFailure`,
`AssertionError`, `ConditionSkipped`, `TtyUnavailable`, `InternalError`.
Retrying cannot help with any of them.

## OnFailure

When a service enters Failed and its definition names an `OnFailure`
service, peinit starts that service — with the exceptions below.
[*cause.entering-failed-starts-the-named-onfailure-service]
`OnFailure` fires on *entry* to Failed, and Failed to Failed is not a
transition, so it fires at most once per failure.
[*cause.onfailure-fires-at-most-once-per-failure]

It does not fire for:
[*cause.onfailure-does-not-fire-for-shutdown-or-definition-breakage]

- **`ShutdownWave`** — no new service starts during shutdown, so a
  fallback would be both impossible and pointless.
- **`ValidationError`, `CycleDetected`, `DependencyFailure`,
  `AssertionError` ** — these are definition or graph breakage rather
  than runtime degradation. A broken definition cannot meaningfully
  trigger a fallback, and the fallback would probably sit in the same
  broken graph.

It fires for everything else, including `ProcessCrash`,
`WatchdogTimeout`, `HealthCheckFailure`, the startup failures, and
`RestartBudgetExhausted` on a non-Critical service.
[*cause.onfailure-fires-for-a-non-critical-budget-exhaustion]

For a **Critical** service exhausting its budget, the reboot takes
precedence and no fallback is started.
[*cause.a-critical-budget-exhaustion-starts-no-handler] The suppression
asks whether a reboot is owed rather than inspecting `ErrorControl`
itself, which is the same question §6.4 puts.

`OnFailure` is for graceful degradation — the main web interface fails,
so start a minimal emergency endpoint. It is not for monitoring or
alerting, which is eventd's job.

### A handler that is already running

If the named handler is already Active — or Reloading, on its way back to
Active — the failure starts nothing.
[*cause.a-handler-already-active-or-reloading-starts-nothing] A running
service has no state to
transition into on being started again, and it is already doing the job
the failure would have asked of it, so the start resolves as satisfied:
no operation is created, and no loop-guard chain entry is recorded for a
cascade that did not happen.
[*cause.a-satisfied-handler-start-records-no-chain-entry]

This is the same answer the control interface gives an administrator who
starts an already-running service, and it is deliberately the same test.
A handler in any other state is started normally — so a Oneshot handler
that ran for an earlier failure runs again for a new one, and a handler
that has since failed is restarted.
[*cause.a-handler-in-any-other-state-is-started-normally]

### The loop guard

An `OnFailure` handler can fail and carry its own `OnFailure`, so a
misconfiguration where A's handler is B and B's is A could run forever.
peinit bounds the chain originating from one failure two ways: it tracks
the set of services already started as handlers for that failure and
will not start one already in the set,
[*cause.a-handler-already-in-the-chain-is-not-started-again] and it will
not follow the chain past a fixed depth of 16.
[*cause.the-chain-is-not-followed-past-a-depth-of-sixteen] When either
trips, peinit records a `peinit.on-failure.suppressed` event naming
which (`outcome.reason` `cycle` or `max-depth`), and stops.
[*cause.a-suppressed-loop-records-an-audit-event]

A chain entry is retired two ways.

**The handler stopped needing tracking.** Completed, Inactive, Skipped, or
Abandoned — it ended, gave up, or became unkillable.
[*cause.a-chain-entry-is-retired-when-the-handler-ends]

**The handler arrived.** It has been dependent-satisfying for its
`RestartWindow` — the same window that resets a restart budget (§6.4).
[*cause.a-chain-entry-is-retired-when-the-handler-holds-for-restartwindow]
That is peinit's existing test for a service having genuinely taken over
rather than merely come up, and it is what ends the originating failure:
a later failure of that handler is a new one, starting a fresh chain.

The distinction matters in both directions. Without the second rule, a
handler that runs healthily for days holds a slot forever, so a
degradation path handing off through several healthy layers exhausts its
own depth budget by working. But retiring on `Active` alone would break
the guard entirely: `Readiness=Alive` reports Active the instant the
process spawns, so two services naming each other as handlers would hand
off forever, with no delay and no cost — an `OnFailure` start is not a
restart and spends no restart budget. Only a handler that *held* is
treated as having arrived.

## The logging contract

A transition into Failed, Skipped or Abandoned is written to the
console, naming the service and its cause: what failed, and why.
[*cause.a-transition-into-a-bad-state-is-written-to-the-console] There
is no per-transition event — a transition is visible through the
`peinit.job.*` and `peinit.operation.*` events that carried it, not as a
record of its own.
[*cause.there-is-no-per-transition-event]

Cryptic failure messages are a defect. A reboot loop caused by a
configuration error with an opaque message is the worst outcome the
system has, and the cause taxonomy exists so that the "why" is never a
guess.
