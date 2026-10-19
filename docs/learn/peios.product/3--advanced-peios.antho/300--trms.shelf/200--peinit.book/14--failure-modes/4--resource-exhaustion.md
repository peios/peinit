---
title: Resource Exhaustion
description: Descriptors, processes, memory,  and disk running out — how each reaches single-threaded PID 1, and what the OOM killer does.
---

peinit is single-threaded PID 1, so most resource pressure reaches it as
a failure at a syscall rather than as slowness.

## Descriptors

peinit holds a descriptor per supervised process (a pidfd), two per
supervised process for output pipes, one per armed timer, one per
control connection, two per jobs connection (the socket and the peer's
pidfd), one per submitted job's output sink, plus the sockets, the
epoll instance and the signalfd.
[*exhaust.the-descriptors-peinit-holds] Control connections are bounded
in all by `MaxControlConnections` (256), which is set for descriptors
rather than for fairness: what keeps one caller from taking them is
`MaxControlConnectionsPerUser` (§10.1). A submission also holds its
prepared token and attached descriptors from acceptance until the launch
takes them, which is bounded by `MaxJobsPerSubmitter` times the
descriptor cap per message.

`EMFILE` or `ENFILE` from `pipe2` or `clone3` fails the start with
`ParentSetupFailure` — a restart-eligible cause, so a service that
failed because the system was momentarily out of descriptors gets
another go.
<!-- lint-disable-next-line line-length -->
[*exhaust.a-descriptor-exhaustion-at-launch-is-a-restart-eligible-parentsetupfailure]

The pre-start check helper's result descriptor and pidfd are both
unregistered from the event loop and closed, so a service using
filesystem conditions costs no descriptors beyond the start itself.
[*exhaust.a-filesystem-condition-leaks-no-descriptors]

## Processes

`EAGAIN` from `clone3` — the PID limit — is also `ParentSetupFailure`
and restart-eligible.
[*exhaust.a-pid-limit-at-launch-is-a-restart-eligible-parentsetupfailure]

The one path that forks outside the launch machinery is the timer
last-run write, one child per firing of a persistent timer (§9.2).
[*exhaust.the-timer-last-run-write-is-the-only-fork-outside-the-launch-path]
The children `_exit` as soon as the write returns and peinit matches
each one's exit status back to its write, so a failure is reported
rather than discarded — but the fork itself is real and happens on every
firing.

The child closes inherited descriptors numbered 3 or above before
opening the registry key, so a blocked write does not keep PID 1's
pidfds, event-loop handles, sockets or service pipe ends open. It keeps
the standard streams and exits when its write completes. Cleanup
failure is reported like a failed timestamp write (§9.2). Driving the
write through an asynchronous LCS path instead would remove the fork
entirely; there is no such path today.

## Memory

`ENOMEM` from `clone3` behaves like the others.
<!-- lint-disable-next-line line-length -->
[*exhaust.a-memory-exhaustion-at-launch-is-a-restart-eligible-parentsetupfailure]

peinit's own memory is bounded by design in the places that could
otherwise grow without limit: the pre-eventd buffer has a fixed size and
drops its oldest, there is no outbound queue for log delivery, terminal
jobs and operations are dropped rather than retained, and neither has a
history structure.

Nor do the two structures that once did. A graph execution context and
its operation associations are retired on the maintenance turn after
every member reaches a terminal state (§7.3), so a boot or an on-demand
start costs nothing lasting.
[*exhaust.graph-execution-contexts-are-retired] And an `OnFailure` chain
entry is cleared once its handler has held for a `RestartWindow` (§6.3),
so a resident handler does not permanently occupy a slot of that
failure's depth budget.
[*exhaust.an-onfailure-chain-entry-is-cleared-once-the-handler-holds]

## Disk

A full root filesystem shows up in three places. The boot attempt
counter cannot be written, which peinit treats as a counter of zero and
continues — a failure to record an attempt is not itself worth
escalating. The random seed cannot be saved at shutdown, which is
recorded and does not block the shutdown.
<!-- lint-disable-next-line line-length -->
[*exhaust.an-unsaveable-random-seed-is-recorded-and-does-not-block-the-shutdown]
And registryd cannot write, which is registryd's problem and reaches
peinit as a Critical service failing.

## The OOM killer

An `ErrorControl=Critical` service is marked OOM-immune, with
`oom_score_adj` at `-1000`; everything else is left at the default
(§5.4). A Critical service is one whose loss reboots the machine, so
letting the OOM killer pick it would turn memory pressure into a reboot.

peinit itself is PID 1 and the kernel will not choose it.
