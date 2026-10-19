---
title: Transitions
description: Every transition peinit performs — anything absent from the table is not performed — plus reset from Abandoned.
---

Every transition peinit performs. Anything not listed here is not
performed. [*trans.a-transition-absent-from-the-table-is-not-performed]

| From | To | Trigger |
|---|---|---|
| Inactive | Starting | Start command, dependency resolution, or a timer trigger. |
| Inactive | Skipped | A condition check failed. |
| Inactive | Failed | Validation, assertion, cycle,  or dependency failure — before any process existed. |
| Starting | Active | Readiness. Simple only: `READY=1` received, or the process exists under `Readiness=Alive`. |
| Starting | Completed | Oneshot exited successfully. Without `RemainAfterExit` it passes through, releasing dependents, then goes Inactive. |
| Starting | Skipped | A condition failed after the activation had already entered Starting. |
| Starting | Failed | An assert failed after entering Starting; or a timeout, hook failure, setup failure, or an exit before readiness. |
| Starting | Backoff | A startup failure with the restart policy allowing a retry and budget remaining. |
| Starting | Stopping | An explicit stop canceling an in-progress start. A *restart* on a Starting service is queued rather than canceling. |
| Starting | Failed | The shutdown wave SIGKILLed a starting service. |
| Active | Reloading | `ExecReload` issued, or SIGHUP delivered. |
| Active | Stopping | Explicit stop, conflict eviction, a bound dependency stopping, or shutdown. |
| Active | Backoff | A crash, a watchdog timeout, or health check failure, with a restart allowed and budget remaining. |
| Active | Failed | The same three, with `RestartPolicy=Never` or the budget exhausted. |
| Active | Inactive | A Simple service exited successfully and `RestartPolicy` is not Always. Cause `CleanExit`. |
| Active | Backoff | A Simple service exited successfully and `RestartPolicy=Always`. Cause `CleanExitRestart`. |
| Reloading | Active | Reload resolved: `READY=1`, the detection window expiring, the extended wait expiring, or the reload command exiting — success or failure. |
| Reloading | Stopping | The same triggers as Active to Stopping. The reload is canceled. |
| Reloading | Backoff | The main process exited during the reload, with a restart allowed. |
| Reloading | Failed | The same, with no restart available. |
| Stopping | Inactive | The process exited after an explicit stop or a shutdown. |
| Stopping | Failed | The process exited after a conflict eviction or a bound dependency stopping. |
| Stopping | Abandoned | SIGKILL sent and `main/` still populated after the post-kill deadline. |
| Stopping | Starting | An explicit start queued against a service that is still stopping. Cause `ExplicitStart`. |
| Backoff | Starting | The backoff delay elapsed. Cause `RestartPolicy`. |
| Backoff | Inactive | An explicit stop canceled the pending restart. |
| Backoff | Skipped | The backoff delay elapsed and the relaunch's pre-start checks skipped the service: its terminal is now held (§11.6), or a condition do not holds. |
| Backoff | Failed | The backoff delay elapsed and peinit could not execute the relaunch. Cause `InternalError`. |
| Starting, Active, Reloading, Stopping | Failed | peinit could not carry out a step on the service and contained the error to it (§8.2). Cause `InternalError`. |
| Completed | Inactive | `RemainAfterExit=0` and dependents released; or an explicit stop; or shutdown clearing it. |
| Completed | Starting | A start command or timer trigger re-running the Oneshot. |
| Failed | Starting | An explicit start, a bound dependency recovering, or a timer trigger. |
| Failed | Inactive | A reset command clearing the state. |
| Failed | Abandoned | A service SIGKILLed by the shutdown wave whose cgroup stayed populated past the post-kill deadline. |
| Abandoned | Inactive | A reset command. |
| Skipped | Inactive | A reset command, or an explicit start clearing Skipped before it re-evaluates the conditions. |

## Things the table settles

**A restart never passes through Failed.** A restart-eligible failure
goes to Backoff, waits, and goes to Starting.
[*trans.a-restart-never-passes-through-failed] Failed is reached only
when there will be no retry: `RestartPolicy=Never`, an invalid policy
for the cause, or an exhausted budget. This is why `OnFailure` (§6.3),
which fires on entry to Failed, does not fire on each retry — only when
the service finally fails out.
[*trans.onfailure-does-not-fire-on-each-retry]

**A clean exit is not a crash.** A Simple service exiting zero goes to
Inactive under cause `CleanExit`, consulting neither the restart policy
nor the budget. [*trans.a-simple-clean-exit-goes-to-inactive-under-cleanexit]
It goes to Backoff only under `RestartPolicy=Always`,
and then with the distinct cause `CleanExitRestart`, so status and
events say plainly that the process succeeded and was restarted by
policy rather than that anything went wrong.
[*trans.a-clean-exit-under-always-goes-to-backoff-as-cleanexitrestart]

**A forced stop remembers why.** Stopping to Failed carries the cause
from the transition that started the stop — `ConflictEviction` or
`BindsToPropagation` — rather than a generic failure.
[*trans.stopping-to-failed-carries-the-cause-that-started-the-stop] A
service that lost a conflict and a service whose binding target went
away are distinguishable afterward, which is what makes
bound-dependency recovery (§7.1) possible at all.

**A restart detours through Inactive.** The stop leg of an
administrator's restart ends in Inactive, and the start leg begins from
there. [*trans.an-administrative-restart-detours-through-inactive] The
two legs meet within one turn of the event loop, and peinit answers no
request mid-turn, so no query ever sees the service Inactive in between.
[*trans.a-restarts-pass-through-inactive-is-not-observable]

**A crash before readiness can be retried.** A Simple process that exits
before signaling readiness is a `ProcessCrash` from Starting, and is
restart-eligible like any other, rather than a terminal startup failure.
[*trans.a-pre-readiness-exit-is-a-restart-eligible-processcrash]

**A process can outlive its service's state.** Most states are reached by
the main process exiting, but not all of them: a watchdog timeout, a
health check escalation and the post-kill give-up all move a service on
while its process is still running. The exit that follows arrives for a
service in Backoff, Abandoned or another state that does not expect
one.

peinit records that exit and performs no transition. The service keeps
the state it had, any pending restart deadline stands, and the console
reports it: [*trans.an-exit-in-an-unexpected-state-performs-no-transition]

```text
peinit: service <service> main process exited in state <state>; no action taken
```

This is deliberately not a fatal condition. A supervised service
producing an unexpected sequence costs that service; it does not cost
the machine. [*trans.an-unexpected-exit-does-not-cost-the-machine]
Treating it as a broken invariant of the runtime loop meant
one crash-looping service dropped PID 1 into recovery mode and killed
every session on the box.

## Abandoned

Resetting an Abandoned service re-checks its `main/` sub-cgroup. If it
has finally emptied, peinit cleans up the whole service tree — `main/`,
`hooks/`, `health/`, then the root — and transitions to Inactive.
[*trans.a-reset-of-an-emptied-abandoned-service-cleans-up-and-goes-inactive]

If it is still populated, peinit leaves the cgroup leaked, transitions
to Inactive anyway, and returns this warning in the operation
acknowledgment:
<!-- lint-disable-next-line line-length -->
[*trans.a-reset-of-a-populated-abandoned-service-warns-and-leaves-the-cgroup-leaked]

```text
abandoned main cgroup for service <service> is still populated after
reset -- cgroup remains leaked; underlying D-state process requires
investigation
```

The warning is also written to the console, so a reset issued without
reading the response still leaves a trace of the still-leaked cgroup.
[*trans.the-reset-warning-is-also-written-to-the-console]

The re-check targets `main/`. Note that the two paths into Abandoned
probe different cgroups: an explicit stop checks `main/`, while the
shutdown wave checks the service root.
[*trans.the-two-paths-into-abandoned-probe-different-cgroups]
