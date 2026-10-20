---
title: Find Service Logs
type: how-to
description: Open service logs, match output to one run, diagnose missing lines, and understand which records survive.
related:
  - peios/services-and-jobs/execution-environment
  - peios/services-and-jobs/jobs-and-operations
  - kernel/internals/kacs/auditing/overview
  - kernel/internals/kacs/auditing/event-families-and-delivery
  - peios/boot/boot-and-boot-modes
---

In **Services Manager**, select the service and choose **Logs…**. It opens
[Event
Viewer](~peios/reading-records/event-viewer) with
the service's lines and the output of its hooks and health checks, newest
first and updating as they arrive.

For terminal queries, use [evctl](~peios/evctl/using-evctl). peinit forwards
output; **eventd** stores and queries the records it receives.

## Find the failed run

1. Run `svctl status <service>` and note its state, cause, current job ID, and
   warnings.
2. Open **Logs…** and inspect the time around the failure. Hook and
   health-check output can explain a start or health failure even when the
   main process wrote nothing.
3. Match the **job GUID** to isolate one execution. A restart has a new job
   ID.
4. For earlier attempts, look for `peinit.job.started`, `peinit.job.ended`,
   and the operation events in eventd. The live job or operation can already
   have expired.

Log origins identify the producer: the service name, a hook such as
`jellyfin/ExecStartPre[0]`, or `jobs/<id>` for a submitted job. A log's
stream tells you stdout or stderr, not whether the line necessarily describes
an error.

## Capturing output

peinit captures stdout and stderr, including hooks and health checks, and tags
lines with the origin, stream, timestamp, and job GUID. A submitted job cannot
opt out of forwarding; `--output` requests an additional live copy.

> [!WARNING]
> A service with `TTYPath` sends all three standard streams to its terminal.
> Its output is **not captured** for eventd. Check the terminal or change the
> service's output arrangement if you need a log record.

Pipe wiring and tagging are described in the [output-flow
reference](~peios/reference/peinit/output-handling/service-output-flow#capturing-output).

## Logs and events have different limits

Service logs and structured events take separate paths:

| Record | Path | What to expect |
|---|---|---|
| Service stdout/stderr | peinit's output buffer and eventd's log socket | Best-effort; lines can be truncated or dropped under load. |
| Job, operation, access-check, and failure events | KMES, then eventd | Independent of the log socket. KMES holds a finite in-memory record; eventd supplies stored history. |

Do not assume an event has survived a reboot merely because it was emitted.
The kernel ring is not persistent, and it can overwrite older events if
consumers fall behind. See [KMES failure
modes](~kernel/internals/kmes/failure-modes) for those limits
and [Auditing](~kernel/internals/kacs/auditing/overview) for event storage and queries.

## The pre-eventd buffer

Before eventd is available, peinit holds output in memory, by default up to
`PreEventdBuffer=1048576` bytes. Overflow drops the oldest lines. The buffer
is replayed when eventd is ready, but it is not persistent storage and cannot
guarantee a complete boot log.

The [buffer
reference](~peios/reference/peinit/output-handling/service-output-flow#the-pre-eventd-buffer)
explains the early-boot path.

## Flood protection

A `[truncated]` marker means a line exceeded `MaxLogLineLength` (8192 bytes by
default). Output delivered faster than peinit can read can block the service's
writes at its pipe; downstream delivery to eventd is loss-tolerant.

If a submitted job's live output has gaps, look for
`peinit.job.output.dropped`. That reports loss in the extra output copy, not
necessarily loss in eventd; query eventd separately. Ordinary log delivery
remains best-effort there too.

See the [registry
limits](~peios/services-and-jobs/registry-key-reference#operational-parameters)
before changing buffer sizes and the [TRM flood-control
detail](~peios/reference/peinit/output-handling/service-output-flow#flood-protection)
for the implementation.

## The eventd handoff

When eventd reaches Active, peinit replays buffered output and forwards new
lines. If eventd exits, peinit resumes buffering and repeats the handoff when
it returns. Missing log lines around that interval can reflect buffer overflow
or lossy delivery; an empty query alone does not prove the service wrote
nothing.

Access policy can also hide records from a query. Check [who can read
what](~peios/reading-records/stored-records) if the expected
origin is absent. The [handoff
reference](~peios/reference/peinit/output-handling/service-output-flow#the-eventd-handoff)
describes transport behavior.

## Console output

peinit writes boot progress, shutdown progress, recovery entry, and critical
failures to `/dev/console`. Service output is not echoed there by default. A
terminal-attached service is the exception because it writes directly to its
terminal.

`peios.quiet` can suppress peinit's console messages, and suppressed messages
are dropped rather than saved for later. See [console noise
settings](~peios/boot/boot-and-boot-modes#console-noise-peios-quiet)
when investigating a boot problem.

## Where to start

For symptom-based next steps, use
[Troubleshooting](~peios/services-and-jobs/troubleshooting). For finding older
executions or actions, use [Jobs and
operations](~peios/services-and-jobs/jobs-and-operations). For query tools,
use [Event
Viewer](~peios/reading-records/event-viewer) or
[evctl](~peios/evctl/using-evctl).
