---
title: Flood Protection
description: The three bounds that stop a noisy service starving the event loop, and where loss is allowed and where it is not.
---

A noisy service cannot be allowed to starve the event loop. Three bounds
apply.

| Key | Default | Minimum | Meaning |
|---|---|---|---|
| `Machine\System\Init\MaxLogLineLength` | 8192 | 256 | Bytes per line before truncation. [*flood.the-max-log-line-length-key] |
| `Machine\System\Init\MaxLogBufferPerService` | 65536 | 4096 | Bytes buffered per service pipe before backpressure. [*flood.the-max-log-buffer-per-service-key] |
| `Machine\System\Init\LogReadBytesPerEvent` | 16384 | 512 | Bytes drained from one pipe per readable event. [*flood.the-log-read-bytes-per-event-key] |

A value below the minimum is **not** honored and does **not** fail the
boot: the compiled-in default is used and a warning is logged naming the
key, the configured value and what was used instead.
[*flood.a-below-minimum-value-keeps-the-default-and-warns] The same rule
covers `PreEventdBuffer` (§11.2), and it is the rule peinit already
applies to the equivalent kernel command-line knobs — a typo in a
logging knob must not decide how the machine boots, and must not be
silent either.

`MaxLogBufferPerService`'s minimum is the one with an external cause:
`F_SETPIPE_SZ` will not go below one page and rounds up regardless, so a
smaller number in the registry would describe a pipe that does not
exist. The other minimums are engineering judgment — the point below
which the mechanism stops working rather than working differently.

A line exceeding `MaxLogLineLength` is truncated and marked
`[truncated]`, with the content trimmed so that content and marker
together come to exactly the limit.
[*flood.an-overlong-line-is-truncated-to-exactly-the-limit] Everything
up to the next newline is then suppressed rather than emitted as a
second line. [*flood.the-remainder-of-a-truncated-line-is-suppressed]

`MaxLogBufferPerService` is applied as the pipe's own capacity through
`F_SETPIPE_SZ` [*flood.the-per-service-bound-is-the-pipes-own-capacity],
which is a literal reading of "bytes buffered per
service pipe before backpressure" — the kernel does the buffering and
the bound is where it belongs.

`LogReadBytesPerEvent` bounds one readable event rather than one loop
iteration. [*flood.the-read-budget-bounds-one-readable-event] A turn
with several ready pipes reads up to the budget from each, bounded
overall by how many events one epoll wait returns.

<!-- lint-disable-next-line line-length -->
## Where loss is allowed and where it is not [*flood.nothing-is-dropped-at-the-pipe]

At the pipe stage, peinit does not drop. It keeps reading within its
budget and appends every complete line; only `EAGAIN`, end of file, or
an error stops the read. Backpressure through the pipe is the
flow-control mechanism between a service and peinit, and dropping there
would replace it with silence.

Downstream of the pipe, delivery is loss-tolerant by design. The
pre-eventd buffer drops its oldest entries when full, and eventd's
datagram socket can drop records under load. The no-silent-drop
guarantee covers *reading the pipe*, not delivery to eventd.
[*flood.delivery-downstream-of-the-pipe-is-loss-tolerant]

Audit events are exempt from all of it. They go through KMES.

## Event loop fairness [*flood.signals-are-handled-at-the-highest-priority]

No source can starve the loop. Signals are handled at the highest
priority — SIGCHLD reaping and shutdown handling take precedence over
everything else in every iteration — with the shutdown deadline timer
next and every other source below that, ties broken by arrival order.

The power button shares the top priority with signals
[*flood.the-power-button-shares-the-top-priority], on the reasoning
that someone physically pressing it is asking for the same class of
attention.
