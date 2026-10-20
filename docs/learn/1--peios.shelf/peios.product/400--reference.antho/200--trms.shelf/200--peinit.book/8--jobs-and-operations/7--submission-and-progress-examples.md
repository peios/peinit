---
title: Submission and Progress Examples
description: Submitted-job identity conveyance, definition fields, descriptor passing,  and status/progress messages.
---

This reference expands the [submitted-job operator
workflow](~peios/services-and-jobs/jobs-and-operations#submitted-jobs). Job
implementation is in §8.5; transport is in §10.7 and the submission protocol
is PSPU §7.

## What a job runs as

There is **no identity field**. A submitter cannot name an identity for its
job; it can only run the job as an identity it already holds:

- **Nothing attached** — the job runs as the submitting *process's own primary
  token*, exactly as a child of the submitter would. This is the common case
  for a tool or a service running something as itself.
- **A token attached** — the submitter attaches a token to the submit message,
  and the kernel (through [KACS](~kernel/internals/kacs/access-check/the-dacl-walk)) gates the
  attach: it verifies the submitter could act as that identity, at that
  impersonation level, at that moment. peinit turns the attached token into
  the job's primary token. This is how `backupd` runs a backup as its client,
  or a logon service starts a session as the user who authenticated — the
  same [impersonation](~kernel/internals/kacs/impersonation/overview) machinery, with the
  kernel as the only judge. A job whose session must convey Delegation needs
  the submitter to set its jobs connection to Delegation level first; a job
  started at Impersonation cannot be raised later.

A token below Impersonation level, or one peinit cannot duplicate, is refused
with `BAD_TOKEN`. peinit never refuses a token because of *who* it names —
that decision was the kernel's.

Two identities are recorded on every job, and the view shows both: the
**submitter** (who asked — the identity the connection was made under) and the
**identity** (who it runs as), plus the identity's **logon session**.

## The definition

A submission carries the whole job in one message: `image_path` (absolute;
required), `arguments`, `environment`, `working_directory` (default `/`),
`description`, `timeout` (seconds; `0`, the default, is no limit),
`stop_timeout` (default 10 s), `readiness` (`none` or `notify`, to wait for
`READY=1`), `readiness_timeout` (default 30 s), `success_exit_codes`, a list
of named `descriptors` to hand the job, an `output` flag, and optionally a
`security_descriptor` for the job itself. There are deliberately **no policy
fields** — no restart, dependencies, health check, or trigger. A program that
needs those is a service.

peinit does not check that the program exists or is executable when it accepts
the submission; that is decided at `execve`, by the job's own token, and a
program that cannot be run becomes a *failed job* rather than a refused
submission. The submit is answered only once the job has **left `created` ** —
the process confirmed its exec, or starting it failed. A `readiness: notify`
job is answered when it is *running*, not when it is ready; a submitter that
wants to block until readiness follows with a `wait`.

Descriptors a submitter attaches are handed to the job from descriptor 3 with
`LISTEN_FDS`, `LISTEN_FDNAMES` and `LISTEN_PID` set — the same convention a
service uses to adopt [stored
descriptors](~peios/services-and-jobs/execution-environment), so a program
written for one adopts them from the other unchanged. This is what makes a
session job possible: the submitter keeps one end of a `socketpair` and
attaches the other.

## The job view

Whether you ask on the jobs socket or with `job status`, you get the same
view: the job `id`, `type` (`submitted`), `state` and `cause`, `submitter`
and `identity` SIDs, `logon_session`, `description`, `image_path`, `pid`
(while running), `ready` (`null` for a `readiness: none` job, otherwise
whether `READY=1` has arrived), `status_text`, `progress`, the exit code or
signal, and the `created_at` /`started_at`/`ended_at` timestamps.

`status_text` and `progress` come from the job itself, through the same
[sd_notify](~peios/services-and-jobs/service-types) socket a service uses. A
job sends `STATUS=Backing up /data/media` for a human-readable line and
`PROGRESS=` in one of three forms:

| Form | Meaning | Shown as |
|---|---|---|
| `PROGRESS=N` | Counting, with no end in sight. | A rising count. |
| `PROGRESS=N/` | Counting toward an end that exists but is not yet known. | A rising count, awaiting a bound. |
| `PROGRESS=N/T` | `N` of `T`. | A bar. |

`PROGRESS_UNIT=bytes|items|percent` says what the numbers are. peinit keeps
the latest accepted values and emits a `peinit.job.status.reported` event when
they change, rate-limited to at most one per job per second so a chatty job
cannot flood the event stream — the *view* always has the latest value
regardless.
