---
title: Control Client Examples
description: Control-socket JSON framing, service and timer status fields, job views and short-lived operation results.
---

These examples connect the [svctl operator
guide](~peios/services-and-jobs/controlling-services) to the control
interface. PSPU §4 owns the wire contract; the socket and dispatch mechanisms
are in §10.1 and §10.2.

## How the control interface works

The socket speaks **newline-delimited JSON**: one request object per line, one
response object per line. A request and its success response look like this:

```json
{"command": "start", "service": "jellyfin", "wait": true}
{"status": "ok", "operation_id": "a1b2c3d4-...", "service": "jellyfin", "state": "active", "cause": "explicit_start", "warnings": []}
```

Two properties are worth knowing even if you only ever use `svctl`:

- **Every command is access-controlled.** By default, anyone who is signed in
  can connect. When you connect, peinit captures your
  [token](~peios/services-and-jobs/identity-and-privileges) from the kernel
  and runs [AccessCheck](~kernel/internals/kacs/access-check/the-dacl-walk) against the target
  service's descriptor for *every* command. There is no "trust localhost," no
  override. Who can do what is the subject of [Who can manage a
  service](~peios/services-and-jobs/who-can-manage-a-service).
- **Lifecycle commands create
  [operations](~peios/services-and-jobs/jobs-and-operations).** A `start`
  /`stop`/`restart`/`reload`/`reset` returns an `operation_id` — a GUID you
  can poll. Conflict resolution between concurrent commands happens at the
  operation layer, which is why two simultaneous `start` s merge instead of
  colliding.

## Reading status

`status` returns the full picture for one service:

```json
{
    "status": "ok",
    "service": "jellyfin",
    "display_name": "Jellyfin",
    "description": "Media server for the living room.",
    "state": "active",
    "cause": "explicit_start",
    "status_text": "Listening on port 8096",
    "progress": null,
    "current_job": {"id": "a1b2...", "type": "service_main", "pid": 1234, "started_at": "...", "identity": "jellyfin-svc"},
    "current_operation": {"id": "e5f6...", "type": "start", "source": "admin"},
    "health": "healthy",
    "uptime_seconds": 86400,
    "definition_removed": false,
    "warnings": [],
    "timers": [],
    "granted": ["query_status", "start", "stop", "interrogate"]
}
```

- `display_name` and `description` are the definition's, or `null`.
- `state` and `cause` are the
  [lifecycle](~peios/services-and-jobs/the-service-lifecycle) pair — read them
  together.
- `status_text` is the latest `STATUS=` string the service sent through
  sd_notify (`null` if never sent; cleared on each restart).
- `progress` is how far the service last said it had got, from `PROGRESS=` and
  `PROGRESS_UNIT=`:
  `{"current": 3, "total": 10, "bounded": true, "unit": "items"}`, with
  `total` and `unit` `null` when it didn't say them. `null` if it never sent
  one; cleared on each restart.
- `current_job` and `current_operation` are the [job and
  operation](~peios/services-and-jobs/jobs-and-operations) GUIDs, or `null`.
- `health` is `healthy`, `unhealthy`, `unknown`, or `null` (no health
  check).
- `definition_removed` is `true` when the definition was deleted but an
  instance is still [draining](~peios/services-and-jobs/defining-a-service).
- `warnings` lists leaked sub-cgroups and other operator-relevant notices.
- `timers` has one entry for each [timer
  trigger](~peios/services-and-jobs/triggers-and-timers), described below.
- `granted` is what **you** can do to this service, as peinit checks it: the
  rights the service's
  [permissions](~peios/services-and-jobs/who-can-manage-a-service) give you,
  out of `query_status`, `start`, `stop` and `interrogate` (reload). A
  program shows a control only where its right is listed, rather than offering
  one that will be refused.

Each entry in `timers` describes one schedule as peinit has it armed:

```json
{"schedule": "*-*-* 02:00:00",
 "scheduled_at": "2026-06-02T02:00:00.000000000Z",
 "fires_at": "2026-06-02T02:07:12.000000000Z",
 "last_fired_at": "2026-06-01T02:03:40.000000000Z",
 "not_armed": null}
```

- `scheduled_at` is the schedule's next occurrence.
- `fires_at` is when the timer will actually fire. It is later than
  `scheduled_at` by the random delay `TimerJitter` drew for this firing.
- `last_fired_at` is when it last fired, or `null` if it has not fired since
  peinit started and has no recorded run.
- `not_armed` is set only for a schedule peinit refused, such as one that
  never comes round. It gives the reason, and the times are then `null`.

Times are in UTC. `svctl status` prints them to the second:

```text
$ svctl status logrotate
logrotate: inactive
cause: clean_exit
timers:
  *-*-* 02:00:00
    next: 2026-06-02T02:00:00Z, firing at 2026-06-02T02:07:12Z after jitter
    last fired: 2026-06-01T02:03:40Z
```

`list` returns a compact summary of every service you can query — services you
lack `SERVICE_QUERY_STATUS` on are **omitted**, not denied:

```json
{"status": "ok", "services": [
    {"service": "jellyfin", "state": "active", "cause": "explicit_start", "health": "healthy", "next_timer_at": null},
    {"service": "logrotate", "state": "inactive", "cause": "clean_exit", "health": null, "next_timer_at": "2026-06-02T02:07:12.000000000Z"}
]}
```

`next_timer_at` is the soonest `fires_at` of the service's timers.
`svctl list` shows it in a **NEXT TIMER** column.

`operation-status` returns one operation by GUID; an unknown or expired GUID
is the `UNKNOWN_OPERATION` error. (Operations are dropped after a short
retention grace once terminal — long enough for a polling client to read the
result, not forever.)

`job status` returns one submitted job's view under `"job"`, and `job list`
returns the views you can query under `"jobs"`:

```json
{"status": "ok", "job": {
    "id": "5f2a...", "type": "submitted", "state": "running", "cause": null,
    "submitter": "S-1-5-21-...-1001", "identity": "S-1-5-21-...-1001", "logon_session": 999,
    "description": "nightly backup", "image_path": "/usr/bin/backup", "pid": 4521, "ready": null,
    "exit_code": null, "exit_signal": null,
    "status_text": "Backing up /data/media", "progress": {"current": 3, "total": 5, "bounded": true, "unit": "items"},
    "created_at": "...", "started_at": "...", "ended_at": null
}}
```

The fields are explained in [Jobs and
operations](~peios/services-and-jobs/jobs-and-operations). A job is retained
for 60 seconds after it ends; after that its GUID is the `UNKNOWN_JOB` error.
Like `list`, `job list` **omits** the jobs you lack `JOB_QUERY` on rather
than denying them.
