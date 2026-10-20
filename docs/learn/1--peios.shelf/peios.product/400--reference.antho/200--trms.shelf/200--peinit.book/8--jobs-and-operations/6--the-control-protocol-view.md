---
title: What a Caller Sees
description: What the job and operation model looks like from outside — identifiers, waiting, and why merging is invisible to the caller.
---

Operations are how a caller observes something taking effect. This
article covers what the model looks like from the outside; the wire
contract itself is PSPU §4.

## Every lifecycle command returns an identifier

A command that creates, merges into, queues, cancels, clears, or executes
an operation returns that operation's identifier. A caller can then poll
it, or block on it. [*protoview.every-lifecycle-command-returns-an-identifier]

Two cases return no identifier, because no operation exists: a command
whose target is already in the state it asks for, and one that has no
effect at all — a stop on an Inactive service. Both return the service's
status instead of an acknowledgment, which is the honest answer, and
which also says what else the caller can do to the service (§10.2).
[*protoview.a-command-with-nothing-to-do-returns-the-status-instead]

Commands that never create an operation — `status`, `list`,
`operation-status` — have their own shapes. So do the three job
commands, which never create an operation either: a submitted job has
no state machine to contend for, so `job-stop` acts on it directly and
answers with the job view.
[*protoview.the-job-commands-never-create-an-operation]

## Waiting

Lifecycle commands block by default until the operation is terminal.
[*protoview.lifecycle-commands-block-until-the-operation-is-terminal] The
exception is `reload`, which returns immediately unless asked otherwise,
because a reload's outcome is often advisory and a caller usually wants
the identifier rather than the wait.
[*protoview.reload-returns-immediately-unless-asked-otherwise]

A waiting connection is not idle and is never closed by the idle
timeout, however long the operation runs. It is bounded by the
operation's own lifetime instead.
[*protoview.a-waiting-connection-is-never-closed-by-the-idle-timeout]

## Merging is invisible

A caller whose command merged receives the surviving operation's
identifier and blocks on that operation's outcome.
[*protoview.a-merged-caller-blocks-on-the-surviving-operations-outcome]
Nothing tells them
they merged, because there is nothing they could usefully do about it.
The consequence worth knowing is that the identifier a caller gets back
can be older than their request, and its `created_at` will be earlier
than when they sent it — which is exactly right, because that is when
the work they are waiting on actually began.
[*protoview.a-merged-callers-identifier-is-older-than-their-request]

## What an operation's result carries

A completed stop or reset carries the resulting service state; a
completed start carries the readiness that satisfied it — `alive
readiness: process started`, `notify readiness: READY=1`, or the exit
code for a Oneshot — rather than a state name. A failed one carries the
failure reason; one that peinit could not execute (§8.2) carries
`internal_error: <cause>`. A merged one carries the survivor's
identifier. A canceled or aborted one carries why.
[*protoview.what-an-operations-result-carries]

For a reload, the result also determines the reload's *mode* — whether
the service confirmed the reload with a `READY=1`, whether peinit is
merely assuming it happened, or whether it outright failed.
[*protoview.a-reloads-result-determines-its-mode]

## The job view

A submitted job is reported in one shape on both sockets, the job view
of PSPU §7.7: identifier, `type` of `submitted`, state, cause,
submitter and identity SIDs, logon session, description, image path,
`pid` while there is a process, `ready` for a `notify` job, exit code
or signal, the retained `status_text` and `progress`, and the three
timestamps. [*protoview.the-job-view-and-its-fields] Every inapplicable
field is present and null, and `pid` is null once the job is terminal —
there is no process to name.
[*protoview.every-inapplicable-field-is-present-and-null] Abandoned is
the exception, and the reason for it: an abandoned job's processes
survived SIGKILL and are still running, so its `pid` is kept.
[*protoview.an-abandoned-jobs-pid-is-kept]

What a submitter sees of its job on the jobs socket is exactly what an
administrator sees of it in `job-status`, but for one field the control
socket adds: `granted`, the job rights that caller holds on it (§10.2).
[*protoview.the-job-view-is-one-shape-on-both-sockets] And `job-list`
is a list of the same views, filtered by `JOB_QUERY` on each (§10.2).
[*protoview.job-list-is-filtered-by-job-query] A `job-stop`
with `wait` registers a wait on the connection like a lifecycle
command's, answered with the terminal view — or `UNKNOWN_JOB`, if the
record was purged before the answer could be sent.
