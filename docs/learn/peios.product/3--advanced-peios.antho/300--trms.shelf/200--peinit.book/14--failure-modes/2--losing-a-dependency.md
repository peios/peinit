---
title: Losing a Dependency
description: What breaks when authd or eventd is unavailable, and what a bound dependency that never becomes satisfying does.
---

## authd

Without authd, no non-SYSTEM service can obtain a token, so every such
start fails with `ParentSetupFailure`. Platform services are unaffected,
since they never take that path.

authd is Critical [*lostdep.authd-is-a-critical-service], so its own
failure eventually reboots the system rather than leaving it in a state
where no user-facing service can start.

## eventd

peinit supervises eventd like anything else, and eventd is Critical.
[*lostdep.eventd-is-a-critical-service] While it is down, peinit
re-enables the pre-eventd buffer (§11.2) and keeps collecting output;
when eventd returns the handoff repeats.

There is a log gap bounded by the buffer size. There is **no** event gap
— audit events land in the KMES ring buffer regardless of eventd's
state, and eventd resumes from the last persisted sequence. A
submitter's output sink (§11.1) is fed regardless, since it is written
from the pipe read rather than from the forwarder.

## A bound dependency

A service that `BindsTo` something is stopped when that something stops,
with cause `BindsToPropagation`, and restarted when it returns, with
cause `BindsToRecovery` and no charge against the restart budget
(§7.1). This is the one dependency relationship that recovers by itself.

A `Requires` dependency crashing does not affect a running dependent at
all. The dependent was ordered after it, not coupled to it.

## A dependency that never becomes satisfying

A dependent blocked on a hard dependency waits until its own operation
lifetime expires, and then fails.
[*lostdep.a-blocked-dependent-fails-when-its-operation-lifetime-expires]
The timeout is measured from when the operation was created rather than
from when it started running, so a service queued behind a slow
dependency can exhaust its `StartTimeout` without ever having attempted
to start
[*lostdep.the-start-timeout-runs-from-when-the-operation-was-created] —
which is the honest answer, in that the caller really has been waiting
that long.
