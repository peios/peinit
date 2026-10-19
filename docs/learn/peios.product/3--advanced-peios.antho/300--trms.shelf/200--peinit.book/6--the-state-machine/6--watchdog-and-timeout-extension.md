---
title: The Watchdog and Timeout Extension
description: The two notification fields a running service uses to adjust the deadlines it is held to, their caps, and where they do not apply.
---

Two notification fields let a running service adjust the deadlines it is
held to. Both are authenticated exactly as any other notification
(§10.5), and both carry microseconds.
[*wdog.both-fields-carry-microseconds]

## The watchdog

`WatchdogTimeout` sets the interval peinit expects `WATCHDOG=1` pings
at. Zero, the default, disables it.
[*wdog.watchdogtimeout-sets-the-ping-interval-and-zero-disables-it]
Missing a ping is a `WatchdogTimeout` cause and takes the ordinary
restart path. [*wdog.a-missed-ping-takes-the-ordinary-restart-path]

A service can change the interval at runtime by sending
`WATCHDOG_USEC=<value>`:

- A value greater than zero updates the interval **and re-arms
  immediately** — the current timer is canceled and a fresh one starts
  from the moment the message was received, rather than the new interval
  applying only from the next ping.
  [*wdog.a-runtime-interval-update-re-arms-immediately]
- A value of zero disables the watchdog entirely, equivalent to
  `WatchdogTimeout=0`.
  [*wdog.a-runtime-value-of-zero-disables-the-watchdog]

The runtime value does not persist. On a restart the interval reverts to
the definition's `WatchdogTimeout` converted to microseconds, and if
that is zero the watchdog starts disabled whatever the previous
incarnation had set.
[*wdog.a-runtime-interval-does-not-survive-a-restart]

`WATCHDOG_USEC` is honored only while the service is Active. A service
that sends it while still Starting — before its own `READY=1` — is
ignored and gets the definition's value.
[*wdog.watchdog-usec-while-starting-is-ignored]

> [!NOTE]
> Runtime watchdog updates suit a service whose phases have genuinely
> different latency. A database engine might want a tight five-second
> watchdog in normal operation and sixty seconds during a compaction
> pass. The service knows its own phases better than whoever wrote its
> definition does.

## Timeout extension

A service can ask for more time during a start, stop or reload by
sending `EXTEND_TIMEOUT_USEC=<value>`.

peinit sets the current phase's deadline to expire that many
microseconds from now.
[*wdog.extend-timeout-usec-extends-the-current-phases-deadline] The
extension **replaces** the deadline rather than adding to it — each
message sets an absolute deadline computed from its own arrival — and
can be sent repeatedly.
[*wdog.an-extension-replaces-the-deadline-rather-than-adding-to-it]

Because it replaces, a small value shortens the remaining time rather
than being ignored, and a value of zero sets the deadline to now.
[*wdog.a-small-extension-shortens-the-remaining-time]

### The caps

The extended deadline cannot exceed four times the phase's base timeout:
[*wdog.the-extended-deadline-is-capped-at-four-times-the-base-timeout]

| Phase | Base | Ceiling |
|---|---|---|
| Starting | `StartTimeout` | `StartTimeout` × 4 |
| Stopping | `StopTimeout` | `StopTimeout` × 4 |
| Reloading | `StartTimeout` | `StartTimeout` × 4 |

A value beyond the cap is clamped, not rejected — the message succeeds
and the deadline becomes the maximum permitted.
[*wdog.a-value-beyond-the-cap-is-clamped-not-rejected] Because the cap is
anchored to when the operation started rather than to the previous
deadline, repeated messages cannot creep past it.
[*wdog.repeated-extensions-cannot-creep-past-the-cap]

During shutdown an additional cap applies: the deadline cannot exceed
the time remaining in the global `ShutdownTimeout`, and where both caps
apply the stricter wins.
[*wdog.during-shutdown-the-stricter-of-the-two-caps-wins]

### Where it does not apply

A message arriving while the service is in a non-transitional state —
Active, Completed, Failed — is ignored. There is no deadline to extend.
[*wdog.an-extension-in-a-non-transitional-state-is-ignored]

During shutdown, as at any other time, an extension moves only a
deadline the service already has. A service still in a transition when
the shutdown begins — starting, reloading, or stopping under an explicit
stop — extends that transition's deadline, and one already stopping
keeps its stop deadline, extension included, into its wave (§12.2). A
service that is Active until its stop wave reaches it has no deadline to
extend before then, so a request it sends earlier is ignored like any
other in a non-transitional state.
[*wdog.before-its-stop-wave-an-active-service-has-no-deadline-to-extend]
A service that expects its stop to take longer sends the request once
its stop has begun.

> [!NOTE]
> Timeout extension is for a service doing variable-duration work in a
> transition — replaying a write-ahead sign in startup, say. It sends
> periodic messages to prove it is making progress; if it stops sending,
> the last deadline fires and peinit escalates as usual. The fourfold
> cap is what stops a buggy service extending forever.
