---
title: Submitted Jobs
description: A process any admitted principal asks peinit to run and supervise — the two doors, the two identities, how a submission becomes a running job, and how it is managed, ends and is forgotten.
---

A submitted job is a supervised process whose definition arrived on the
jobs socket rather than from a service definition. It has no name, no
policy, and no operation: something asked for it, peinit ran it under an
identity the asker was entitled to confer, and it is managed by whoever
the job's own Security Descriptor admits. The protocol is PSPU §7; this
chapter is how peinit implements its side.

Two shapes drive the design, and one mechanism serves both. A **task**
— a backup, a migration — is submitted by a broker on behalf of the
principal it is serving, runs to completion and reports progress on the
way. A **session** — a per-logon host process — is submitted with a
freshly issued logon token and one end of a socketpair, and lives until
something stops it. Nothing in peinit distinguishes them.

## Two doors

The control socket is an administrator's door: its commands change the
state of the system's services, and its descriptor admits the
principals entitled to do that. Submitting a job is a different
permission held by a different population, so it has a different
socket, `/run/services/peinit/jobs.sock` (§10.7), with a different
descriptor. The filesystem says who can submit and who can administer,
separately, and peinit carries no policy of its own about either:
being able to connect to the jobs socket *is* the permission to submit,
and peinit performs no access check before a `submit`.
[*submit.connecting-to-the-jobs-socket-is-the-permission-to-submit]

What a caller can do *to a job* is a third question, answered by the
job's own descriptor. A submitter manages its jobs on the jobs socket;
an administrator observes and stops them from the control socket with
`job-list`, `job-status` and `job-stop` (§10.2). Both doors check the
same descriptor with the same rights.
[*submit.both-doors-check-the-same-descriptor]

## Two identities

Every submission involves two identities, and peinit keeps them apart.

The **submitter** is the connection identity: the peer token captured
once, at accept, exactly as the control socket captures it (§10.1). It
is recorded on the job, it is what the quota counts against, and it is
the owner of the job's descriptor. A submitter that connects while
impersonating is recorded as the principal it was impersonating.

The **job identity** is what the process runs as. It is established in
exactly one of two ways, and the boundary that does it —
`LinuxJobIdentityProvider` — has no third:

- **No token attached.** peinit opens the primary token of the
  connecting *process* through the kernel's handle on it. The handle
  comes from `SO_PEERPIDFD` at accept; the token comes from
  `peios_token_open_process` on that pidfd with `QUERY | DUPLICATE`.
  The job then has the identity a child of the submitter would have
  had — its primary token, with no impersonation inherited
  [*submit.with-no-token-attached-the-job-runs-as-the-peers-primary-token]
  — so a submitter that was impersonating when it connected produces a
  job running as *itself* while being recorded as a submitter under the
  impersonated identity. A submitter that wants the impersonated
  identity on the job attaches it.
- **A token attached.** The kernel delivered a token with the `submit`
  message through `KACS_SCM_TOKEN`, having run the impersonation gates
  with the submitter as installer and recorded on the token the lower
  of the socket's level and the source's own (Peios Kernel TRM §3.5).
  peinit uses that token as it arrived, at `QUERY | IMPERSONATE |
  DUPLICATE`.

Either way the source is duplicated to a **primary** token at its own
impersonation level, with `TOKEN_ALL_ACCESS`, and the duplicate is
what the child installs. The level carries through: a job started from
an Impersonation-level token cannot convey Delegation, and a submitter
that needs it — for a session that will authenticate onward on its
user's behalf — sets its own jobs connection to Delegation before
attaching, because the attach is clamped to the socket's level.

peinit refuses, with `BAD_TOKEN`, a source whose impersonation level is
below Impersonation
[*submit.a-token-below-impersonation-level-is-refused-with-bad-token] —
an Identification-level token cannot pass an
access check and an Anonymous one is not an identity to run a process
as — and a source it cannot duplicate. It refuses nothing because of
*who* the token names. Whether the submitter can run a job as that
identity was the kernel's decision, made when the token was conveyed,
and a second decision inside peinit could only disagree with the first.

There is no identity field in the definition, and no route by which a
plain descriptor passed with `SCM_RIGHTS` becomes a job identity. A
descriptor passed that way is not gated by the kernel — whoever holds
an fd can pass it — and it would be the one path worth attacking.

The prepared token is a raw descriptor held on the job's entry until
the launch takes its own duplicate of it (`materialize_prepared_token`,
§4.1). A submission refused after the token was prepared closes it on
the way out.

## The submission

`submit` runs in this order, and a refusal at any step leaves nothing
behind — no record, no event, no quota consumed, and every descriptor
the message carried closed: [*submit.a-refusal-leaves-nothing-behind]

1. **Parse and validate the definition** against the attachment count
   (below). A malformed definition is `INVALID_ARGUMENTS` before any
   identity work is done.
   [*submit.a-malformed-definition-is-invalid-arguments]
2. **Refuse during shutdown** with `INVALID_STATE`. The other four
   commands keep working while the system goes down.
3. **Establish the job identity**, as above.
4. **Check the quota.** peinit counts the submitter's live jobs — those
   not yet terminal — by submitter SID against
   `Machine\System\Init\MaxJobsPerSubmitter`, default 64. SYSTEM
   (`S-1-5-18`) is exempt. At the bound the answer is
   `QUOTA_EXCEEDED`. [*submit.the-quota-counts-a-submitters-live-jobs] A
   job stops counting the moment it is terminal, not when its retained
   record is dropped.
   [*submit.a-job-stops-counting-against-the-quota-when-it-is-terminal]
5. **Build the job's descriptor**: the one the submission supplied in
   SDDL, used as given, or the default (below).
6. **Create the record, the entry and the launch queue entry**, in one
   `SupervisorWork` transaction.

The job is two things in peinit's state. The **job record** is the same
`JobRecord` every fork gets (§8.1), with `job_type: Submitted`, no
service, no operation, the job identity's user SID as
`resolved_identity`, and a cgroup id under `/sys/fs/cgroup/peinit/jobs/`
named by the job's GUID. The **entry** is what the jobs system holds
beside it: the submitter, the job identity's user SID and logon
session, the definition, the descriptor, the prepared token, the
attached descriptors with their names, the output sink, readiness and
the retained `STATUS` and `PROGRESS`, any stop in progress, and once the
job is terminal its outcome and retention deadline. Records are dropped
when terminal, as every job's is; entries outlive them for the grace
period, which is how a terminal job still answers `status`.

The `submit` is answered when the job **leaves `created` ** — on exec
confirmation or on launch failure — and not before.
[*submit.the-submit-is-answered-when-the-job-leaves-created] The
submitter's connection holds a pending wait until then, reads nothing
further, and is not idle. A running job's answer carries a duplicate of
the job's pidfd as `SCM_RIGHTS`; a failed launch is still
`"status": "ok"`, with a terminal view whose `cause` says why.
[*submit.a-failed-launch-is-still-status-ok] A pidfd that cannot be duplicated
is answered `INTERNAL_ERROR` rather than a view with the handle silently
missing — the submitter was promised one, and `status` gives it another
chance.

### The definition [*submit.the-definitions-defaults-and-validation]

| Field | Default | Validation |
|---|---|---|
| `image_path` | required | Absolute, non-empty, no NUL. Not checked for existence: `execve` decides that, as the job's identity. |
| `arguments` | `[]` | Strings without NUL. |
| `environment` | `{}` | Keys non-empty, no `=`, no NUL; values no NUL. |
| `working_directory` | `/` | Absolute. |
| `description` | `""` | |
| `timeout` | `0`, no limit | Non-negative integer, seconds. |
| `stop_timeout` | 10 | Positive integer, seconds. |
| `readiness` | `none` | `none` or `notify`. |
| `readiness_timeout` | 30 | Non-negative integer, seconds. |
| `success_exit_codes` | `[]` | Integers 0–255. |
| `descriptors` | `[]` | One name per injected descriptor; non-empty, no `:`. |
| `output` | `false` | Requires at least one attached descriptor. |
| `security_descriptor` | the default | SDDL with an owner and a DACL. |

`descriptors` has to match the attachments exactly: its length plus one
if `output` is set equals the number of descriptors on the message.
[*submit.descriptors-must-match-the-attachment-count]
The combined size of `arguments` and `environment` is bounded at 2 MiB,
below what `execve` accepts.
[*submit.arguments-and-environment-are-bounded-at-two-mib] Nothing here
restarts, depends, probes, or schedules; a program that needs any of that
is a service.

### The descriptor

Unless the submission supplied one, the descriptor is built by
`PeiosSystemAccessChecker` from the submitter SID: owner and group the
submitter; a DACL granting `JOB_ALL_ACCESS` to the submitter, to SYSTEM
and to Administrators; nothing else; and a SACL auditing every refusal,
for everyone, so a denied command on the job is recorded (§8.4).
<!-- lint-disable-next-line line-length -->
[*submit.the-default-descriptor-grants-the-submitter-system-and-administrators]
The job identity is granted
nothing. A process cannot, by virtue of running as U, see or stop a job
that runs as U — a submitter that wants the principal to see its own
session says so in the descriptor it supplies.

A supplied descriptor is used as given, with no default entries added —
its SACL included, so one with none has its denials recorded by no one.
A submitter that omits itself has locked itself out of its own job, and
peinit does not prevent that.
[*submit.a-supplied-descriptor-is-used-as-given] The descriptor is fixed
at submission.

| Right | Bit |
|---|---|
| `JOB_QUERY` | 0x0001 |
| `JOB_STOP` | 0x0002 |
| `JOB_SIGNAL` | 0x0004 |
| `JOB_ALL_ACCESS` | 0x0007 |

The generic mapping is read → `JOB_QUERY`, write and execute →
`JOB_STOP | JOB_SIGNAL`, all → `JOB_ALL_ACCESS`. Every command on either
door checks against this descriptor, the submitter included, naming the
job to KACS in the check's audit context; a denial is answered
`ACCESS_DENIED`, and the descriptor's SACL decides whether KACS records
it (§8.4). [*submit.a-denial-is-answered-access-denied]

## Launch

Queued submissions launch from the work pump, one per pump step, through the
ordinary child path (§5.3, §5.4) with three differences. The token
source is `LaunchTokenSource::Prepared`: the launch takes its own
duplicate of the prepared token through `materialize_prepared_token`,
and the entry's copy is closed once the launch has it, whatever the
outcome. The attached descriptors ride as inherited descriptors, placed
from 3 upward with close-on-exec cleared and `LISTEN_FDS`,
`LISTEN_FDNAMES` and `LISTEN_PID` set, exactly as the fd store hands
descriptors back to a service (§10.6).
<!-- lint-disable-next-line line-length -->
[*submit.attached-descriptors-are-placed-from-three-upward-with-the-listen-variables]
And the environment is built as
a service's is — the compiled-in base, the global layer, then the
submission's `environment` in the place a definition's own variables
occupy, then the protocol variables last, so a submitter cannot
override `NOTIFY_SOCKET` or the `LISTEN_*` set (§5.5).
[*submit.a-submitter-cannot-override-the-protocol-variables]

The cgroup is `/sys/fs/cgroup/peinit/jobs/<guid>`, a tree of its own
rather than a service's.
[*submit.the-job-runs-in-a-cgroup-of-its-own-under-peinit-jobs]

Exec confirmation and failure arrive through the setup pipe as for any
job. A parent-side failure — no token, no cgroup, no fork — takes the
record from Created straight to Failed with cause
`parent_setup_failure`; a child-side failure between fork and exec, or
a failed exec, is `pre_exec_failure`.
[*submit.a-launch-failure-is-parent-setup-failure-or-pre-exec-failure]
Both close everything the entry
still held and try to remove the cgroup. On success the record is
Running, the runtime registers the job's pipes with origin
`jobs/<guid>`, and adopts the output sink if there is one (§11.1).

A `stop` on a job still queued cancels it: the launch entry is removed,
the record fails before start with the stop's cause, and the answer is
the terminal view.
[*submit.a-stop-on-a-queued-job-cancels-it-before-it-runs] A `stop` that
lands while setup is pending kills the cgroup and lets setup completion
find the stop already recorded.

## While it runs

A submitted job speaks the notification socket like a service, with
one difference in how it is believed: peinit routes a datagram to a
service or to a submitted job by a pure PID lookup first — does any
service's current main job carry this PID — and only then verifies the
sender against that job's pidfd. A submitted job has no activation
generation and no replacement, so the generation step does not apply.
The sender is verified exactly once whichever way it was routed
(§10.5).

What the fields do to a submitted job:
<!-- lint-disable-next-line spelling -->
[*submit.the-notification-fields-a-submitted-job-may-send]

| Field | Effect |
|---|---|
| `READY=1` | For a `notify` job not yet ready: `ready` becomes true and the readiness timeout is disarmed. Otherwise ignored. |
| `STATUS=` | Retained as `status_text`. |
| `PROGRESS=` | Parsed as §4.19's three forms; retained as `progress`. A value outside the forms, `T` of zero or `N` above `T` is dropped, never repaired; the rest of the datagram is applied. |
| `PROGRESS_UNIT=` | `bytes`, `items` or `percent`, retained; anything else dropped. |
| `STOPPING=1` | Recorded, so a later stop sends no termination signal. |
| Everything else | Ignored: a job has no reload, no watchdog, no fd store. |

A datagram that changed `status_text`, `progress`, or the unit is due a
`peinit.job.status.reported` event carrying the retained values (a
`verbose` type, written only when the emission policy turns it on) —
but at most one per
job per second, measured from the last event emitted for that job, so
a sender updating a thousand times a second produces one event a
second while the view is current on every query.

Deadlines are held on the entry and folded into the one lifecycle
deadline timer the event loop arms, as `SubmittedJob` kinds:
[*submit.the-submitted-job-deadlines]

| Deadline | Due | Action |
|---|---|---|
| `Timeout` | `started_at + timeout` | Begin a stop with cause `timeout`. |
| `ReadinessTimeout` | `started_at + readiness_timeout`, for a `notify` job not yet ready | Begin a stop with cause `readiness_timeout`. |
| `StopKill` | stop requested + `stop_timeout` | SIGKILL the job's cgroup. |
| `PostKill` | kill + `PostKillTimeout` | Abandon (below), or re-arm if the cgroup is empty but the exit not yet reaped. |
| `CgroupCleanup` | terminal + `PostKillTimeout` | Retry removing a cgroup that was busy when the job ended. |

A job being stopped has only its stop deadline; a deadline that fires
for a job that has already ended does nothing.

## Stopping

Every stop — a submitter's `stop`, an administrator's `job-stop`, a
timeout, shutdown — goes through `begin_submitted_stop`: record the
cause and the kill deadline, then SIGTERM the main process through its
pidfd unless the job has already sent `STOPPING=1`.
[*submit.a-stop-sigterms-the-main-process-and-arms-the-kill-deadline] A
second stop on a job already stopping changes nothing and does not
restart the deadline; a stop on a terminal job is a no-op answered with
the unchanged view.
<!-- lint-disable-next-line line-length -->
[*submit.a-second-stop-changes-nothing-and-a-stop-on-a-terminal-job-is-a-no-op]

At the kill deadline the cgroup is killed.
[*submit.at-the-kill-deadline-the-cgroup-is-killed] At the post-kill
deadline, a cgroup still populated means the process survived SIGKILL:
the record is abandoned, the cause becomes `process_unkillable`, the
cgroup is leaked and reported as `peinit.cgroup.leaked`, naming the job
rather than a service, and supervision stops.
[*submit.a-process-that-survives-sigkill-is-abandoned-as-process-unkillable]

`signal` is the raw mechanism, deliberately: one signal, by number, to
the main process through its pidfd, on a running job only.
[*submit.signal-acts-on-a-running-job-only] `SIGKILL`
this way leaves whatever the job spawned and produces a failed job with
`exit_signal` set and a null cause.
<!-- lint-disable-next-line spelling -->
[*submit.a-signalled-kill-produces-a-failed-job-with-a-null-cause] A
submitter that wants the job *ended* uses `stop`.

## Ending

On reap, exit code 0 or one in `success_exit_codes` completes the job;
any other code, or a signal, fails it.
[*submit.success-is-exit-zero-or-a-code-in-success-exit-codes] Then
whatever the job left in
its cgroup is killed and the cgroup removed; a busy cgroup schedules
one `CgroupCleanup` retry, and a cgroup still busy after that is
reported leaked. The prepared token, any attached descriptor the launch
never took, and any sink the runtime never adopted are closed. The sink
the runtime *did* adopt closes when the job's last pipe closes.

`cause` records what peinit decided, and stays null when the process
ended of its own accord.
[*submit.cause-is-null-when-the-process-ended-of-its-own-accord] A stop
whose process handled SIGTERM and
exited 0 is `completed` with cause `explicit_stop`: peinit asked, the
process agreed, and both facts are recorded.
[*submit.a-stop-the-process-handled-completes-with-cause-explicit-stop] A
stopped job that died to the signal is `failed` with `exit_signal` set
and the same cause.
[*submit.a-stopped-job-killed-by-the-signal-fails-with-the-same-cause]

The record is dropped at once, as every terminal job's is (§8.1), and
`peinit.job.ended` carries it. The entry is retained for 60 seconds so a
submitter polling for the outcome finds it, and then purged by
operation maintenance; `status` on a purged or never-existent
identifier is `UNKNOWN_JOB` either way.
[*submit.a-purged-or-unknown-identifier-is-unknown-job-either-way] A
terminal job whose cgroup cleanup is still pending is not purged until
the retry has run.

## Shutdown

When a shutdown begins, every live submitted job is stopped at once,
with cause `shutdown` and no ordering between them or against the
service waves — a job has no dependencies to order by.
[*submit.every-live-job-is-stopped-with-cause-shutdown] A job still
queued for launch is canceled with the same cause. The shutdown is not
finished while a live job remains: a job's reap advances shutdown
progress exactly as a service's does, and the global timeout kills
every live job's cgroup along with the remaining services (§12.2).
`submit` is refused throughout; the other four commands keep answering,
because a submitter watching its jobs go down has a reason to keep
looking.

## Clients

`svctl job list|status|stop` are the control socket's job commands, and
`svctl job submit|wait|signal` speak the jobs socket, submitting as the
caller's own primary token with `--fd NAME=FD` for descriptors and
`--output` for a sink. [*submit.the-two-halves-of-svctls-job-commands]
libpeinit exposes the same two halves:
`peinit_job_status`, `peinit_job_list` and `peinit_job_stop` on the
control client, and `<peinit/jobs.h>` — a `peinit_jobs_t` with
`peinit_job_submit`, `peinit_jobs_status`, `peinit_jobs_wait`,
`peinit_jobs_stop`, and `peinit_jobs_signal` — with
`peinit_response_take_pidfd` to claim the handle a submit returned.
