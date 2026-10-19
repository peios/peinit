---
title: Wiring
description: peinit is not a logging system but it holds the pipes at birth — where a service's output goes, how it is tagged, and terminal-attached services.
---

peinit is not a logging system — eventd stores, indexes, and queries
logs. But peinit holds the pipes at birth. It decides where a service's
output goes, and it has to cover the window before eventd exists.

## The pipes [*output.a-service-gets-a-stdout-pipe-and-a-stderr-pipe]

Before exec, peinit creates a pipe for stdout and one for stderr with
`pipe2(O_CLOEXEC)`. The child's streams are redirected onto the write
ends; peinit keeps the read ends and watches them with epoll.

The blocking discipline is asymmetric, deliberately
[*output.the-blocking-discipline-is-asymmetric]:

- The parent's **read** ends are non-blocking. PID 1 cannot afford to
  block on a read.
- The child's **write** ends keep ordinary blocking semantics.

That second half is what makes backpressure work. When a service
produces faster than peinit consumes, the kernel pipe buffer fills and
the service's own `write()` blocks, so the service slows down. Making
the write ends non-blocking would turn that into `EAGAIN` failures in
the service, converting a flow-control mechanism into an error the
service has to handle.

The child's stdin is redirected to `/dev/null`.
[*output.stdin-is-redirected-to-dev-null] peinit provides no
interactive input channel; a service needing input obtains it explicitly
— through a socket, or a stored descriptor — never through inherited
stdin.

The epoll instance itself is close-on-exec and is never inherited.
[*output.the-epoll-instance-is-never-inherited]

## Tagging [*output.every-line-is-tagged]

peinit reads output line by line and tags each line with:

- the origin,
- the stream — stdout or stderr,
- a `CLOCK_REALTIME` timestamp,
- the job's identifier.

The origin is the service name for a main process. For a hook it is the
service name, the hook kind and its index — `jellyfin/ExecStartPre[0]`.
Reload commands are `<service>/ExecReload` and health checks
`<service>/HealthCheck`. A submitted job, which has no service, is
`jobs/<guid>`. [*output.the-origin-names-the-producer]

Health check output is captured, which is usually the only way to find
out why a health check failed. [*output.health-check-output-is-captured]

<!-- lint-disable-next-line line-length -->
## The output sink [*output.a-submitted-jobs-output-is-recorded-unconditionally]

A submitted job's output is recorded like any other's; that is
unconditional, and eventd is always the primary destination. A
submitter can additionally ask for a copy by attaching a descriptor
with `output: true` (§8.5). When the job's pipes are registered, the
runtime adopts that descriptor as the job's **sink**: it is put in
non-blocking mode, and every line read from either pipe is written to
it as the line followed by a newline, untagged, in the order the lines
were read. [*output.the-sink-receives-untagged-lines-in-order] The two
streams interleave; the sink gets what the job wrote and nothing else.

The copy is best-effort and the record is not. A write that would
block drops the line for the sink only, counts it, and on the first
drop for that job emits one `peinit.job.output.dropped` event — one per job,
however many lines follow — so a submitter that sees a gap can learn
it caused one. [*output.a-blocked-sink-write-drops-one-line-and-reports-once]
Any other write failure closes the sink at once. The
sink is otherwise closed when the job's last pipe closes, after the
last line has gone to it, and a sink attached to a job that never got
pipes is closed without a write.
[*output.the-sink-is-closed-when-the-pipes-are]

<!-- lint-disable-next-line line-length -->
## Terminal-attached services [*output.a-terminal-attached-service-is-not-captured]

A service with a `TTYPath` has all three streams on its terminal, and
both pipe pairs are closed in the child (§5.4). Its output is not
captured at all — it goes to the terminal, which is what asking for one
means.

peinit opens the terminal with read data, write data **and read
attributes**. [*output.the-terminal-is-opened-with-read-attributes] The
last matters because the descriptor is inherited by
every process the service starts, and a KACS handle answers `fstat`
only if that right is on it: without it a session can read and write
its terminal but not name it — `ttyname()` fails, `tty` reports "not a
tty", and a program choosing `TERM` cannot tell a serial line from a
virtual console. `isatty()` is an ioctl and works either way, which is
why the gap is easy to miss.
