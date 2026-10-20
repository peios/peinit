---
title: Evaluation and Arming
description: A timer is a trigger rather than a service type — how one is armed, how it fires, and what multiple triggers do.
---

A timer is a trigger, not a service type. A service with a
`timer:<schedule>` trigger is an ordinary Simple or Oneshot service that
peinit starts on a schedule.
[*evalt.a-timer-is-a-trigger-on-an-ordinary-service]

## Arming

At boot, once the service graph is loaded, and whenever timer
configuration changes, peinit computes the next firing time for every
active trigger and arms a timerfd for it. Each trigger gets its own
descriptor and its own computation.
[*evalt.every-active-trigger-gets-its-own-armed-descriptor]

A disabled service gets neither a registration nor a firing.
[*evalt.a-disabled-service-is-neither-registered-nor-fired]

A schedule that fails to parse, or whose next occurrence cannot be
computed, fails that trigger. Every other timer arms normally, and what
did not arm is reported to the console and in the service's `status`.
[*evalt.a-bad-schedule-fails-only-its-own-trigger-and-is-reported]
This matches how graph
validation already treats an invalid schedule (§7.4), so the outcome does
not depend on which of the two caught it.

The next-occurrence search looks ten years ahead and then gives up.
[*evalt.the-next-occurrence-search-gives-up-after-ten-years] A
schedule can parse and still match nothing — `*-02-30`, or a fixed year
already past — and the horizon turns that into a prompt error against
the one service rather than a very long walk. Ten years clears the
sparsest schedule that is genuinely meaningful: `*-02-29` skips a
century year not divisible by 400, so it can run eight years dry.

## Firing [*evalt.a-firing-is-classified-from-the-services-type-and-state]

```text
handle_timer(service, trigger):
    // 1. Decide what the firing means, from the service's state.
    match (service.type, service.state):
        (Oneshot, Active | Starting):
            service.pending_timer = true      // at most one
        (Simple,  Active | Starting):
            record the firing; no action
        (Oneshot, Inactive | Completed | Failed):
            create_operation(Start, service, source = Timer)
        (Simple,  Inactive | Failed):
            create_operation(Start, service, source = Timer)

    // 2. Record when it fired.
    write the last-run timestamp to the registry   // asynchronously

    // 3. Re-arm.
    next = next_occurrence(trigger.schedule, now) + random(0, TimerJitter)
    arm an absolute CLOCK_REALTIME timerfd for next
```

Every other state — Backoff, Stopping, Reloading, Abandoned, Skipped —
records the firing and does nothing.
[*evalt.a-firing-in-any-other-state-does-nothing]

A timer can outlive its service until the next reload. A service whose
definition is deleted while it runs carries on, and is discarded when it
stops, and nothing re-plans the timers then. A firing for a service absent
from the table does nothing, records no last run, and is not armed again;
the next reload leaves it out.
[*evalt.a-firing-for-a-service-that-is-gone-does-nothing] It is
never an error: an error out of a firing takes PID 1 into recovery.

No last run is recorded for a service whose definition has been
deleted, whether it is still running or already gone: there is no key
to record it in, and writing one would make the key again.
[*evalt.no-last-run-is-recorded-for-a-deleted-definition]

The last-run write happens in a forked child so that the event loop
never waits on the registry
[*evalt.the-last-run-write-happens-in-a-forked-child]
— which matters because the registry is
served by registryd, a service peinit supervises, so a synchronous write
would let a wedged registryd stall PID 1.

Before opening its own registry key, the child closes every inherited
descriptor numbered 3 or above. It keeps standard input, output, and error;
PID 1 retains its own supervision descriptors. Both the single-timer and
per-trigger layouts open a fresh key for their timestamp write. A cleanup
failure makes the child exit unsuccessfully and follows the same warning
path as a failed registry write.
[*evalt.timer-writer-closes-inherited-descriptors-before-opening-its-key]

The parent returns immediately with the child's pid, and remembers it.
When the child is reaped, peinit matches its exit status back to the
write; a failure is reported:

```text
peinit warning: recording the last run of timer <schedule> for service
<service> failed; it will run catch-up again after a reboot
```

The write stays best-effort — nothing is retried and nothing is failed
over it [*evalt.a-failed-last-run-write-is-reported-and-nothing-is-retried]
— but a persistent timer whose timestamp never lands runs its
catch-up on every boot, and that is otherwise a symptom with no thread
to pull. The outstanding-write table is bounded, so a child that somehow
escapes reaping cannot grow it.

## Oneshot pending runs

A Oneshot that fires while it is already running sets a flag rather than
queueing an operation. When it next reaches Inactive or Completed,
peinit immediately creates a start operation and clears the flag.
[*evalt.a-oneshot-firing-mid-run-becomes-one-pending-run]

Multiple firings during one run collapse into a single pending run.
There is no queue, and the flag is per service rather than per trigger —
a service with three timers that all fire during one long run still gets
exactly one catch-up.
[*evalt.multiple-firings-during-one-run-collapse-into-one]

## Multiple triggers

Triggers on one service are independent: each has its own timerfd, its
own next-firing computation, and its own last-run history. Only the
Oneshot pending flag is shared.
[*evalt.triggers-on-one-service-are-independent]

## Reporting

`status` reports every trigger of a service that is not disabled, as it
stands: the occurrence armed, the time it will fire with its jitter, and
when it last fired; or, for a trigger that did not arm, why not (PSPU
§4.14). `list` carries the soonest firing of each service.
[*evalt.status-reports-each-trigger-as-armed]

The figures are the armed ones, given to the supervisor whenever a
trigger is armed: at boot, on a reload that adds or changes the timer
(an unchanged timer keeps its armed deadline), after each firing and
after a clock change. A client cannot compute them itself, because the jitter
is drawn at random when the trigger is armed.
[*evalt.what-is-reported-is-what-is-armed]

When a trigger last fired is held in memory as well as the registry.
It is seeded at boot from the recorded timestamp of a persistent
trigger, set by each firing, including a boot catch-up, and kept across
a reload for the same service and schedule. A definition deleted and
made again between two reloads is the same service and schedule, and
keeps it too. A non-persistent trigger,
which records nothing (§9.3), reports only its firings since peinit
started.
[*evalt.the-last-firing-is-seeded-at-boot-and-kept-across-a-reload]
