---
title: Restart
description: What happens when a restart-eligible cause occurs — the policies, backoff, the budget and when it resets, and exhaustion.
---

When a restart-eligible cause occurs, or a Simple clean exit produces
`CleanExitRestart`, peinit evaluates the restart policy. The outcome is
either Failed, or Backoff followed by Starting.

```text
evaluate_restart(service, cause):
    // 1. Policy.
    if cause is never-restart:
        return STAY_FAILED
    if cause == CleanExitRestart and policy != Always:
        return INVALID_CAUSE_FOR_POLICY
    if policy == Never:
        return STAY_FAILED
    if policy == OnFailure:
        // A termination counts as success only when the cause is a
        // process exit whose code is in SuccessExitCodes. Only
        // ProcessCrash and CleanExitRestart carry an exit code, and
        // CleanExitRestart was handled above. Every other eligible
        // cause has no exit code and is always a failure here.
        if cause == ProcessCrash and exit_code in success_exit_codes:
            return STAY_FAILED
    // A clean terminal logout is not a failure.
    if cause == CleanExitRestart and TTYPath is set:
        consecutive_failures = 0
        return RESTART_AFTER(min(restart_delay, 60))
    // Other Always exits fall through.

    // 2. Budget.
    if consecutive_failures >= restart_max_retries:
        cause = RestartBudgetExhausted
        if error_control == Critical:
            sync and reboot
        return STAY_FAILED

    // 3. Delay.
    delay = min(restart_delay << consecutive_failures, 60)

    // 4. Schedule.
    return RESTART_AFTER(delay)
```

`RESTART_AFTER` puts the service in Backoff for the delay; when it
elapses the service transitions to Starting and the ordinary activation
sequence begins, with its own fresh `StartTimeout`.
[*restart.a-relaunch-gets-a-fresh-starttimeout]

The elapsed backoff timer is consumed when the relaunch is admitted. If a
pre-start check needs an asynchronous filesystem helper, the service remains
in Backoff until that check finishes, retaining its terminal reservation.
The pending check drives progress; the expired timer cannot admit another
restart while it waits.

The relaunch runs the pre-start checks like any other start, and the
machine can have changed while the service waited. If its terminal is
now held (§11.6) or a condition do not holds, the service goes from
Backoff to Skipped, with the cause the check gives, and its pending
operation completes as a skip. A relaunch that peinit itself cannot
execute — its bookkeeping refuses the start — takes the service from
Backoff to Failed under `InternalError` (§6.3), fails the operation
waiting on the restart with the `internal_error` result, and reports
both on the console; it does not cost the machine.

The exit code is available only when peinit observed a process exit, and
it is carried into the evaluation wherever there was one — including the
pre-readiness exit, where a Simple service exits before signaling
`READY=1`. So `SuccessExitCodes` means the same thing on both sides of
readiness: a service that legitimately concludes "nothing to do" during
startup and exits with a listed code is not restarted.
[*restart.successexitcodes-applies-to-a-pre-readiness-exit-too]

Failures with no process exit behind them — a hook that never ran, a
readiness deadline, a dependency failure — carry no code, and the
`SuccessExitCodes` branch cannot apply to them.
[*restart.a-failure-with-no-exit-code-cannot-match-successexitcodes] That
is not a gap: a success code is a statement about how the service's own
process ended.

## The policies [*restart.the-three-policies]

| Policy | Value | Behavior |
|---|---|---|
| Never | 0 | Never restart. The service stays Failed. |
| OnFailure | 1 | Restart on a non-zero exit or a runtime failure. An exit matching `SuccessExitCodes` is not restarted. |
| Always | 2 | Restart on any failure regardless of exit code, and for a Simple service also on a successful clean exit. |

For a **Oneshot** with `RestartPolicy=Always`, a successful exit is not
restart-eligible. `RestartPolicy` governs the response to failures; a
Oneshot that succeeds has done its job. It goes to Completed — and then
Inactive without `RemainAfterExit` — whatever the policy says, and only
a non-zero exit reaches the restart evaluation at all.
[*restart.a-oneshot-that-succeeds-is-not-restarted-whatever-the-policy]
Timer triggers are the mechanism for re-running a Oneshot on a schedule.

## Backoff

The delay doubles on each consecutive failure, starting from
`RestartDelay` and capped at 60 seconds.
[*restart.the-delay-doubles-and-caps-at-sixty-seconds] The arithmetic is
overflow-safe, so a large `RestartDelay` or a long failure run saturates
at the cap rather than wrapping.
[*restart.the-backoff-arithmetic-saturates-rather-than-wrapping]

While a service is in Backoff it is down and does not satisfy
dependents. An explicit `start` creates or merges into a **deferred**
start operation, which honors the remaining delay rather than
short-circuiting it;
<!-- lint-disable-next-line words spelling -->
[*restart.an-explicit-start-during-backoff-honours-the-remaining-delay] a
`stop` cancels the pending restart and takes the service to Inactive.
[*restart.a-stop-during-backoff-cancels-the-pending-restart]

For a terminal-attached service, a successful `Always` exit is a normal
session
logout. It resets the failure counter and uses the base delay (capped at 60
seconds), without spending or exhausting the crash budget. Failed exits and
startup failures still use the ordinary budget and exponential backoff.

## The budget, and when it resets

`consecutive_failures` counts consecutive restart-eligible failures. It
resets to zero **only after the service has stayed Active for
`RestartWindow` seconds**.
[*restart.the-budget-resets-only-after-restartwindow-active] It is not a
count of restarts within a trailing window, and the difference is what
the mechanism turns on.

peinit stamps the moment a service becomes dependent-satisfying, and
clears that stamp on any transition to a non-satisfying state — so a
crash restarts the clock.
[*restart.a-crash-restarts-the-window-clock] The reset fires when the
stamp plus `RestartWindow` is reached with the service still Active.

A service that recovers and stays Active longer than `RestartWindow`
between crashes therefore never exhausts its budget: each crash starts
from a counter of zero.
<!-- lint-disable-next-line line-length -->
[*restart.a-service-healthy-for-a-window-between-crashes-never-exhausts-its-budget]
Only failures recurring faster than the service can sustain a window of
health accumulate.

Two other events also zero the counter, both of which mean the service
is outside a failure run: a clean exit to Inactive, and an
administrative reset.
[*restart.a-clean-exit-or-a-reset-also-zeroes-the-counter] An explicit
stop while in Backoff does not — the accrued failures are preserved.
[*restart.a-stop-in-backoff-preserves-the-accrued-failures]

Because the reset requires the service to be Active, a service that
happens to be Reloading when its window boundary passes misses that
reset and gets it on returning to Active.
[*restart.a-reload-across-the-window-boundary-defers-the-reset]

## Exhaustion

Once `RestartMaxRetries` restarts have happened without the service
sustaining a window of health, the next failure is not restarted: Failed
with cause `RestartBudgetExhausted`.
[*restart.exhaustion-is-failed-with-restartbudgetexhausted] peinit then
applies `ErrorControl`:

- **Normal** — the service stays Failed.
  [*restart.errorcontrol-normal-leaves-the-service-failed]
- **Critical** — peinit syncs the filesystems and reboots immediately.
  [*restart.errorcontrol-critical-syncs-and-reboots]
  The reboot takes precedence over `OnFailure`.

The reboot does not depend on how the budget was exhausted. The paths
that observe a terminal outcome for a running service — the main job
ending, a health check failing or timing out, the watchdog expiring —
raise it directly. Every other route reaches it through a check made
once per runtime turn, which asks a single question of the service
table: is a Critical service Failed with cause `RestartBudgetExhausted`?
That covers a budget exhausted purely by startup failures — repeated
`ReadinessTimeout`, `PreHookFailure`, `ParentSetupFailure` — where a
service that can never get as far as running would otherwise have
settled in Failed with neither escalation.
<!-- lint-disable-next-line line-length -->
[*restart.a-budget-exhausted-by-startup-failures-still-reboots-a-critical-service]

The `OnFailure` suppression asks the same question rather than
inspecting `ErrorControl` itself, so the handler is skipped exactly when
a reboot is owed and never on the strength of one that was not
scheduled. [*restart.the-handler-is-skipped-exactly-when-a-reboot-is-owed]

A failure during a shutdown reboots nothing (§12.2 step 2), and the
handler is not started either — the machine is already going down.
[*restart.a-failure-during-shutdown-neither-reboots-nor-starts-a-handler]
