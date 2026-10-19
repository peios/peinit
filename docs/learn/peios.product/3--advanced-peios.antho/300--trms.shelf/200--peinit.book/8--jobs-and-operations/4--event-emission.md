---
title: Event Emission
description: The structured events peinit emits at every job and operation transition and for its own records, their catalog names, and the emission policy asked before each is built.
---

peinit emits a structured event at every job and operation lifecycle
transition, and for its own records. All of them go into the KMES
kernel ring buffer through `kmes_emit`, encoded as msgpack per the KMES
event-record format (Peios Kernel TRM §2).
[*emit.every-job-and-operation-transition-emits-a-kmes-event]

**There is no event socket.** Structured events are not sent to eventd
over any connection. [*emit.there-is-no-event-socket] eventd consumes
them from the ring buffer, which is why they survive eventd being down,
being restarted, or not existing yet. The only thing peinit sends eventd
over a socket is service output (§11.4), which is a different path with
different guarantees.

## Names and shape

Every type peinit writes is under its platform root, `peinit`, and every
payload is a nested map whose fields sit at their catalog paths
(PGSS §6.4): the job is `object.job.guid`, the service
`object.service.name`, the client that asked `subject.token.sid`, the
result `outcome.success`. A value peinit does not have is an absent key,
never nil; SIDs are written in binary (`bin.sid`), job and operation
identifiers as binary GUIDs (`bin.guid`) that read back as the text
`svctl` shows, enumerated values in kebab-case (`service-main`,
`explicit-start`), and times as nanoseconds since the Unix epoch.
<!-- lint-disable-next-line words spelling -->
[*emit.every-event-is-a-catalogue-type-with-a-nested-payload]
peinit's fragment, `/usr/share/evman/peinit.evman`, describes every type
and field, and the Events reference generates its pages from it.

No event carries a principal's name, peinit's own log wording, or Rust
debug output. What a person reads is on the console; what a consumer
queries is in the fields.

## The emission policy

Before peinit builds an event that is not `essential`, it asks the
emission policy, `Machine\Generic\Events` (PGSS §6.9): a type switched
off is never built, let alone written. With nothing set, a `standard`
type is on and a `verbose` type is off.
[*emit.peinit-asks-the-emission-policy-before-building-an-event]
peinit opens one view of the policy for the life of PID 1; it needs no
registry to open, decides by tier until the registry is readable, and
applies a committed change to the next decision.

Four types are `essential` and never ask: `peinit.boot.downgraded`,
`peinit.recovery.entered`, `peinit.critical-service.failed`, and
`peinit.event.dropped`. Each is rare, and each is the only record of
what it describes. The `verbose` types are `peinit.job.created`,
`peinit.job.status.reported`, `peinit.operation.started`,
`peinit.operation.merged`, `peinit.graph.operation.ended`,
`peinit.config.reload.deferred`, `peinit.notify.status.reported`, and
`peinit.notify.progress.reported`; everything else is `standard`.
Setting `Enabled = 1` on `Machine\Generic\Events\peinit` turns all of
them on.

## Job events [*emit.the-three-job-events]

**`peinit.job.created`** — the job object exists, before its process
does. Carries the job, its type and state, its service and the
activation generation it serves, the operation it serves, the
executable, and the token it will run as.

**`peinit.job.started`** — exec succeeded. Adds the process ID
(`object.process.pid`) and the cgroup (`object.cgroup.path`).

**`peinit.job.ended`** — the process exited or was killed, or the job
ended without starting. Carries the whole record: the terminal state,
`outcome.success` (true exactly for `completed`) with the failure cause
as `outcome.detail`, the exit code or signal, the executable and
arguments, the times it was created and started, its duration, and its
cgroup and cgroup generation.

The token a job runs as is on each once peinit holds it: its user and
group SIDs, and its present and enabled privileges as flags. A service's
token is minted as its process is set up, so its `peinit.job.created`
carries at most the user SID of a well-known identity.

`peinit.job.ended` cuts `object.job.arguments` to 32 KiB of whole
arguments; `object.job.arguments-truncated` and
`object.job.arguments-count` say so.
[*emit.job-ended-cuts-its-arguments-and-says-so] An event the ring
refuses outright is dropped, counted, and replaced by a
`peinit.event.dropped` naming its type, its length, its service and
job, and the error number, and a `[ WARN ]` line reports it; only the
ring refusing that small event is fatal (§8.2). The count of drops is
the console's, and the trail's is the number of those records.
`peinit.internal-error.contained` records an error peinit contained to
one service (§8.2): the step, the service, the job, and whether the
service was failed. The error's text stays on the console and in the
failed operation's result.

A submitted job's lifecycle rides the same three events with no service,
no activation generation and no operation: the keys are absent.
[*emit.a-submitted-jobs-events-carry-no-service-and-no-operation] Two
more are its own:

**`peinit.job.status.reported`** — the job reported `STATUS` or
`PROGRESS`. Carries the job, the submitter's SID, and `notify.status`
and `notify.progress.*` as retained after the datagram. Written at most
once per job per second; a datagram inside the window updates the view
and writes nothing (§8.5).
[*emit.job-status-is-emitted-at-most-once-per-job-per-second]

**`peinit.job.output.dropped`** — the submitter's output sink stopped
draining and peinit began dropping its copy of the job's lines. Once per
job, on the first drop; the record in eventd is unaffected (§11.1).
[*emit.output-dropped-is-emitted-once-per-job]

## Operation events [*emit.the-operation-events-and-what-they-add]

Every operation event carries the operation (`object.operation.guid`),
its type, source and state after the transition, and its service.
An operation a client's command created also carries the client as
`subject.token.sid`; one peinit created itself has no subject.
[*emit.every-operation-event-carries-the-common-fields]

| Event | Adds |
|---|---|
| `peinit.operation.requested` | — |
| `peinit.operation.started` | — |
| `peinit.operation.ended` | `outcome.success`, `outcome.detail`, `object.operation.duration` |
| `peinit.operation.merged` | `object.operation.merged-into.guid` |

`peinit.operation.ended` is one type for the four ways an operation can
end other than merging — completed, failed, canceled while pending,
aborted while running — and `object.operation.state` says which.
`outcome.success` is true exactly for `completed`. A merged operation
has no outcome of its own: the one it merged into decides it.

`object.operation.duration` is measured from the **request**, not from
the start of execution — the same reasoning as the operation timeout —
and is absent on a canceled operation, which never ran.
[*emit.an-operations-duration-is-measured-from-its-request] What a
caller waited is what matters, and queue time is part of it.

The operation's result, in the words the control interface gives a
client (`inactive`, `startup killed by shutdown`), is
`outcome.detail`, whichever way it ended. The control interface's
operation view calls the same text `result` or `error` (PSPU §4).
[*emit.an-operations-result-is-its-outcome-detail]

## Ordering

When one runtime step produces several lifecycle events, peinit emits
them in causal order before committing the retained state for that step.
For a terminal pre-start graph dispatch, the terminal event for the
operation whose outcome satisfied or failed the graph input precedes the
events for the operations that dispatch releases, which preserve graph
dispatch order. [*emit.a-graph-dispatchs-events-are-emitted-in-causal-order]

Every operation in a graph context is requested when the context is
built rather than when its turn comes, so what a release emits is
`peinit.operation.started`.
[*emit.a-graph-contexts-operations-are-requested-when-the-context-is-built]

## Peinit's own records [*emit.audit-records-go-through-the-same-path]

peinit's own records go through the same path:

- `peinit.on-failure.suppressed` when the fallback chain guard trips;
- `peinit.graph.validation.failed` and `peinit.graph.validation.warned`
  for validation findings, one per finding, and `peinit.boot.downgraded`
  for each finding that forced Safe mode;
- `peinit.config.reload.applied` for an explicit `reload-config` and for
  the reload that ends the boot window, and
  `peinit.config.reload.deferred` for a reload during it (§3.7);
- `peinit.notify.rejected` for a refused notification datagram, and
  `peinit.fd-store.rejected` for a refused descriptor;
- `peinit.notify.status.reported`, `.errno.reported`,
  `.exit-status.reported`, `.stopping.reported` and
  `.progress.reported` for the event-emitting notification fields, the
  last at most once a second for each activation (§10.5);
- `peinit.cgroup.leaked` the first time a sub-cgroup is found still
  populated after its post-kill deadline (§5.7);
- `peinit.graph.operation.ended` for a graph member's terminal outcome;
- `peinit.recovery.entered` for the reason peinit dropped to a recovery
  shell (§2.8);
- `peinit.critical-service.failed` and `peinit.service.abandoned` from
  the shutdown path (§12);
- `peinit.service.reload.timed-out` for a reload a service started and
  never confirmed (§6.5).

Records are events rather than logs, and that distinction is the point:
the ring buffer persists from the moment PKM loads, so a recovery during
Phase 1 is captured before the registry exists, let alone eventd.

## Access decisions

peinit writes no event of its own for a refused command. Every access
check it makes goes through KACS AccessCheck, and KACS records the
decision, as `kacs.audit.access.checked`, when the descriptor's SACL asks
for it (PGSS §6.7). [*emit.a-refused-command-is-recorded-by-kacs]
peinit's compiled-in descriptors — the default service descriptor, the
control descriptor, and a submitted job's default — each carry a SACL
auditing every refusal, for everyone, so a denial is recorded unless a
descriptor says otherwise (§4.6, §13.4). A refusal also leaves a
debugging line on the console, written only at `peios.quiet=0`.

So that the record says what was decided on, each check passes KACS an
audit context naming its object, which the kernel copies into the record
beside `fields.attestation.userspace`:
[*emit.every-access-check-names-its-object-in-an-audit-context]

| Checked | Context | The record carries |
|---|---|---|
| A service | `{kind: "service", service: {name}}` | `object.kind` `service`, `object.service.name` |
| A job, on either socket | `{kind: "job", job: {guid}}`, the GUID in binary | `object.kind` `job`, `object.job.guid` |
| The control door | `{kind: "peinit-system"}` | `object.kind` `peinit-system` |

The rights in `access.requested` and `access.granted` are decoded against
`object.kind`: the `SERVICE_*`, `JOB_*`, and `SYSTEM_*` tables (§4.6,
§8.5, §10.2). `job-list` checks every job it might list against that
job's own descriptor, so each job it leaves out is one more decision for
the job's SACL.

peinit holds `SeAuditPrivilege` enabled for this: without it KMES
refuses its events, and the access-check path writes no record (§13.2).
