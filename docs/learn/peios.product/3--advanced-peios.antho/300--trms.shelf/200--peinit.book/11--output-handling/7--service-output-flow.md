---
title: Service Output Flow
description: Capture, early buffering, flow control,  and eventd forwarding behind the operator log view.
---

These notes collect the output-flow explanation behind [finding service
logs](~peios/services-and-jobs/output-and-logging). The preceding sections
define the precise transport and limits. Service-output delivery is
best-effort; KMES events use a separate finite in-memory ring, and eventd
supplies persistence.

## Capturing output

When peinit forks a service it creates pipes for `stdout` and `stderr`,
redirects the child's streams to the write ends, and keeps the read ends —
monitoring them in its event loop. (The child's `stdin` is `/dev/null`; see
[The execution environment](~peios/services-and-jobs/execution-environment).)
It reads output **line by line** and tags each line with:

- the **service name**,
- the **stream** (stdout or stderr),
- a **timestamp**,
- the **[job](~peios/services-and-jobs/jobs-and-operations) GUID**.

The job GUID is the important one: it is what lets a later query return
exactly one execution's output, never interleaved with the run before or after
it. Hook output is captured too, labeled with the hook (for example
`jellyfin/ExecStartPre[0]`), and so is health-check output — invaluable for
diagnosing *why* a check failed. A [submitted
job](~peios/services-and-jobs/jobs-and-operations)'s output is captured the
same way, labeled `jobs/<id>`; its submitter cannot opt out of that, but can
ask for a **copy** by attaching an output sink, to which peinit writes each
line as it reads it. The copy is best-effort — lines the sink cannot take are
dropped for the sink only, while forwarding to eventd continues independently,
and reported once per job as an `output.dropped` event — and the sink never
slows the job or peinit.

## The pre-eventd buffer

Before eventd is running there is nowhere to send logs — its socket does not
exist yet. peinit therefore buffers captured output in a fixed-size in-memory
**pre-eventd buffer** (default 1 MB); when it fills, the oldest entries are
dropped.

In practice the only services that run before eventd are `registryd`, `lpsd`
, `authd`, and `eudev`. The first three are Peios-owned with controlled
output; `eudev` is the real overrun risk (verbose device enumeration), and the
[restart budget](~peios/services-and-jobs/supervision) naturally bounds output
from a crash loop.

## Flood protection

A noisy service must never starve peinit's event loop. Several limits enforce
that:

| Key (`Machine\System\Init\`) | Default | Limits |
|---|---|---|
| `MaxLogLineLength` | 8192 | Bytes per line; longer lines are truncated with a `[truncated]` marker. |
| `MaxLogBufferPerService` | 65536 | Bytes buffered per service pipe before back-pressure. |

Two mechanisms back these up:

- **A per-readable-event budget.** `LogReadBytesPerEvent` bounds the bytes
  drained from one pipe per readable event. A loop turn with several ready
  pipes can read up to that budget from each; see §11.3.
- **Back-pressure, not dropping.** If a service writes faster than peinit
  reads, the kernel pipe buffer fills and the service's own `write()` calls
  block. This is deliberate: the service slows down. While reading the pipe,
  peinit does **not** drop output — the no-silent-drop guarantee covers
  *reading the pipe*. It is only *downstream* (the pre-eventd buffer, and
  eventd's datagram socket) that delivery becomes lossy.

> [!NOTE]
> Signals are never starved. In every loop iteration, child reaping (SIGCHLD)
> and shutdown handling take priority over all other event sources, including
> log reading. A flood of service output can never delay peinit noticing that
> a service died or that the system is shutting down.

## The eventd handoff

When eventd reaches Active, peinit switches from buffering to forwarding:

1. It begins sending to eventd's log datagram socket (path from
   `Machine\System\eventd\LogSocketPath`).
2. It **replays the pre-eventd buffer**, oldest first, preserving each line's
   timestamp and metadata. This replay is best-effort — the records are
   datagrams on a loss-tolerant socket and some can be dropped under load;
   peinit never blocks waiting to deliver them.
3. It switches to real-time forwarding — new output goes out as it arrives.
4. It clears the buffer.

From then on peinit is a pipe relay: read lines, tag them, and forward the
largest ordered batch that fits the smaller of the portable 262144-byte
ceiling and the log socket's send buffer. peinit does not read eventd's larger
local ceiling; see §11.4. It keeps one connected socket and reuses its
encoding buffer, so normal forwarding does not create a socket or output
allocation for every line. The log socket is **non-blocking and
loss-tolerant** — if eventd cannot drain it fast enough the kernel drops
further datagrams silently, because sign ingestion must never exert
back-pressure on the whole system through PID 1. peinit keeps no unbounded
outbound buffer and never blocks on a send to eventd.

If eventd itself crashes after starting, peinit (which supervises it like any
service) notices, **re-enables the pre-eventd buffer**, and repeats the
handoff when eventd comes back. There can be a log gap across the restart,
bounded by the output buffer. Structured events use the independent KMES ring;
eventd can resume reading the events that remain there. The ring can overwrite
old events and does not survive reboot, so eventd supplies persistence rather
than KMES. See [KMES failure
modes](~kernel/internals/kmes/failure-modes).
