---
title: Controlling Services
type: how-to
description: Inspect status, choose a safe service action, follow its result, and use Services Manager or svctl with the required permissions.
related:
  - peios/services-and-jobs/the-service-lifecycle
  - peios/services-and-jobs/who-can-manage-a-service
  - peios/services-and-jobs/jobs-and-operations
  - peios/services-and-jobs/shutdown
  - peios/services-and-jobs/troubleshooting
---

Use **svctl** to inspect and control services from a terminal, or **Services
Manager** on the GXWI desktop. Start by checking the service before issuing a
command that interrupts it.

```sh
svctl list
svctl status jellyfin
```

Use the name from the list in place of `jellyfin`. The command form is
`svctl <command> [service] [flags]`. To investigate a failure, keep the
state, cause, job ID, and warnings, then open the service's
[logs](~peios/services-and-jobs/output-and-logging).

## Reading status

Read **state** and **cause** together, then check health and warnings:

```sh
svctl status jellyfin
svctl list
```

| Detail | How to use it |
|---|---|
| `state`, `cause` | Tell whether the service is running, waiting to retry, skipped, or failed, and why. See [service states](~peios/services-and-jobs/the-service-lifecycle). |
| `status_text`, `progress` | The latest information the service reported. These clear on restart and can be absent. |
| `current_job` | Identifies this execution. Use its GUID to separate its logs from previous runs. |
| `current_operation` | Identifies an action in progress. Poll its ID with `svctl operation-status <id>`. |
| `health` | `healthy`, `unhealthy`, `unknown` (no check result yet), or `null` (no check configured). |
| `warnings` | Notices such as leaked cgroups. Read these before trying another start. |
| `definition_removed` | A deleted definition still has a draining instance. `stop` still works; `start`, `restart`, and `reload` do not. |
| `granted` | Your query, start, stop, and reload rights. A visible service does not imply permission to change it. |
| `timers` | Next scheduled occurrence, actual firing after jitter, last firing, or a reason the timer could not be armed. |

An `Active` service has met its configured readiness check. With
`Readiness=Alive`, that only confirms the process exists; it does not prove
the application is serving requests.

For a scheduled service:

```text
$ svctl status logrotate
logrotate: inactive
cause: clean_exit
timers:
  *-*-* 02:00:00
    next: 2026-06-02T02:00:00Z, firing at 2026-06-02T02:07:12Z after jitter
    last fired: 2026-06-01T02:03:40Z
```

`svctl` prints timer times in UTC, to the second. Services Manager shows them
in local time. `svctl list` has a **NEXT TIMER** column for each service's
soonest firing. Jitter is already included in the actual firing time.

`list` omits services you cannot query. `job list` does the same for jobs. An
operation ID expires after its terminal retention grace; a submitted job is
retained for 60 seconds after it ends. For older results, look in
[eventd](~peios/reading-records/stored-records). The [TRM client
examples](~peios/advanced-peios/peinit/the-control-interface/operator-client-examples#reading-status)
retain the full JSON views and field formats.

## Manager

On a GXWI desktop, **Services Manager** does the same from a window. It lists
every service, shows what the selected one is doing (its state and why, its
process, the account it runs as, how long it has been running, anything it has
reported), and offers Start, Stop, Restart, Reload and Reset. The same
commands are on each row's right-select menu. Type in **Find a service** to
narrow the list by name or description.

Services Manager is another client of the same control socket, so it has
exactly your authority and no more:

- **A command is offered only where it will act.** A button is available only
  if you hold the right the command needs (see the table below) *and* the
  service is in a state where the command does something (see [the command ×
  state matrix](#the-command-state-matrix)). Otherwise the button is
  unavailable. The details pane says which commands you can use on the
  selected service.
- **Commands do not block the window.** A command is sent without waiting.
  Services Manager follows its operation and reports the result, such as "SSH
  server was restarted." or the reason it failed.
- **Hidden services are listed, not silently dropped.** `list` omits the
  services you cannot query. If you can read the service definitions in the
  registry, Services Manager still lists those services, marked **Not yours to
  see**, and says how many there are.
- **If peinit does not let you connect at all,** Services Manager says so. By
  default the control socket admits everyone who is signed in, but it can be
  locked down. Any definitions you can read are still listed, and no command
  is offered.
- **Its definition opens in a window of its own,** from **Edit definition…**
  in the details pane or on a row's menu. **New service…** on the bar defines
  one. Both are covered in [Defining a
service](~peios/services-and-jobs/defining-a-service#creating-and-changing-a-definition),
  with `svctl definition`, which does the same from a terminal.
- **Who can control a service, and who can change its definition,** open from
  the details pane in the permissions editor. They are covered in [Who can
  manage a
service](~peios/services-and-jobs/who-can-manage-a-service#changing-them-from-the-desktop).
- **What it has logged opens in Event Viewer,** from **Logs…** in the details
  pane or on a row's menu. The window shows the service's lines and those of
  its hooks and health checks, newest first and as they are written. See
  [Event
  Viewer](https://learn.peios.org/peios/desktop-event-viewer/event-viewer/).
- **Timers show when they run.** The **Next run** column shows when each
  service's soonest [timer](~peios/services-and-jobs/triggers-and-timers)
  fires. The details pane lists each timer under its schedule, with its next
  run and its last run, in the machine's local time. A timer with jitter says
  that its run is put back by a random delay, and the next run shown already
  includes that delay. A timer peinit could not arm says why it never runs.
  These are peinit's own figures, read from `status`, not worked out from the
  definition.

peinit doesn't announce state changes, so Services Manager asks it for the
services' state every two seconds. Press **F5**, or select **Refresh**, to ask
straight away. What the registry holds is different. That covers which
services are defined, who can control them, and what you can change. Services
Manager watches the `Services` key for changes and reads it again only when
something changes, so a change made elsewhere, from svctl or another window,
shows at once.

## Before changing a service

- **Start** begins the full start sequence and can start dependencies.
- **Stop** interrupts the service. A `BindsTo` dependent stops with its
  target; a `Requires` dependent that is already running does not. See
  [Dependencies](~peios/services-and-jobs/dependencies).
- **Restart** stops and starts as one operation, replacing the current
  process. Use it when a definition change needs a new process.
- **Reload** asks the running program to re-read its own configuration.
  `reload-config` instead asks peinit to re-read service definitions. Neither
  promises to apply launch-only settings to an existing process.
- **Reset** clears Failed, Abandoned, or Skipped to Inactive; it does not
  start the service or repair the cause.

After the action finishes, run `svctl status <service>` again. If it failed or
only returned an advisory reload result, follow the [troubleshooting
guide](~peios/services-and-jobs/troubleshooting) before repeating it.

## Service commands

| Command | Does | Required right |
|---|---|---|
| `start` | Run the service through its full start sequence. | `SERVICE_START` |
| `stop` | SIGTERM, then SIGKILL after `StopTimeout`. | `SERVICE_STOP` |
| `restart` | Stop then start, as one operation. | `SERVICE_STOP` + `SERVICE_START` |
| `reload` | Re-read configuration (`ExecReload`, or SIGHUP). | `SERVICE_INTERROGATE` |
| `reset` | Clear `Failed`/`Abandoned`/`Skipped` → `Inactive`. | `SERVICE_STOP` |
| `status` | Report state, cause, PID, uptime, health, current job and operation, warnings, and when each timer fires. | `SERVICE_QUERY_STATUS` |
| `list` | List services, their states,  and their next timer firing (filtered to what you can query). | (per-service `SERVICE_QUERY_STATUS`) |

```sh
svctl start jellyfin
svctl restart jellyfin
svctl reload nginx
svctl reset failed-migration     # clear a Failed state without starting
```

## Wait semantics

By default, a lifecycle command **blocks until its operation reaches a
terminal state** — `start` waits for Active (or Failed), `stop` waits for
Inactive, and so on. Pass `--no-wait` to get the `operation_id` back
immediately and poll with `operation-status` instead.

| Command | Default | Waits for |
|---|---|---|
| `start` | wait | Active (Simple) / Completed or Inactive (Oneshot), or Failed |
| `stop` | wait | Inactive |
| `restart` | wait | the successful start target, or Failed |
| `reload` | **no-wait** | (with `--wait`) the Reloading state to resolve |
| `reset` | immediate | — |

`reload` is the exception — it returns immediately by default, because a
reload can have no observable completion. With `--wait`, the response carries
a `mode`:

| `mode` | Meaning |
|---|---|
| `confirmed` | The service signaled `READY=1` (and, for a command reload, the command exited 0). |
| `advisory` | The reload was issued and the detection window elapsed without explicit confirmation. |
| `failed` | The `ExecReload` command exited non-zero or timed out. **The service stays Active** — a failed reload never takes down a running service. |

The detection window is a **fixed 2 seconds** — it is built in and is *not*
configurable through the registry. If the service says nothing within that
window, the reload resolves as `advisory` and the service stays Active.

> [!NOTE]
> A connection blocked on a `--wait` operation is *not* counted as idle, so it
> is not closed by `ConnectionTimeout`. It stays open until the operation
> resolves, bounded by the operation's own timeout (for example `StartTimeout`
> ).

## System commands

| Command | Does | Required right |
|---|---|---|
| `shutdown <type>` | Graceful [shutdown](~peios/services-and-jobs/shutdown). `type` is `poweroff`, `reboot`, or `halt`. | `SYSTEM_SHUTDOWN` |
| `boot` | Report this boot's mode, reason, and confirmation status. | `SYSTEM_QUERY_STATUS` |
| `reload-config` | Re-read *all* definitions and rebuild the graph (atomic). | `SYSTEM_RELOAD_CONFIG` |
| `operation-status <id>` | Report the state of an operation by GUID. | `SERVICE_QUERY_STATUS` on its target |

```sh
svctl shutdown poweroff
svctl reload-config
svctl operation-status a1b2c3d4-...
```

## Job commands

[Submitted jobs](~peios/services-and-jobs/jobs-and-operations) are reachable
from two places, and `svctl job` covers both. Querying and stopping a job is
*administration* and goes over the control socket; submitting a job, waiting
on it, and signaling it belong to its submitter and go over the **jobs
socket** at `/run/services/peinit/jobs.sock` (`--jobs-socket` overrides the
path). Every job command is checked against the **job's own** descriptor,
which by default admits the submitter, SYSTEM, and Administrators.

| Command | Socket | Does | Required right |
|---|---|---|---|
| `job list [filters]` | control | List the jobs you can query. Filters: `--submitter SID`, `--identity SID`, `--logon-session N`, `--state created\|running\|completed\|failed\|abandoned`; all given filters must match. | per-job `JOB_QUERY` (others omitted) |
| `job status JOB_ID` | control | The job view. | `JOB_QUERY` |
| `job stop JOB_ID` | control | SIGTERM, then SIGKILL of the job's cgroup after its `stop_timeout`. Waits for the job to end by default; `--no-wait` returns the view as it then is. | `JOB_STOP` |
| `job submit [options] IMAGE [ARG...]` | jobs | Submit `IMAGE` as a job running as **your own primary token**, and print its view once it is running (or has failed to start). With `--wait`, wait for it to end too. | connect to the jobs socket |
| `job wait [--for ready\|terminal] JOB_ID` | jobs | Block until the job is terminal (default) or has sent `READY=1`, then print the view. `--for ready` on a job without a readiness protocol is `INVALID_STATE`. | `JOB_QUERY` |
| `job signal JOB_ID SIGNAL` | jobs | Send one signal (a number, or a name like `SIGUSR1`) to the job's main process. Only the main process, only while `running`. | `JOB_SIGNAL` |

```text
$ svctl job list --state running
JOB       STATE    SUBMITTER              IDENTITY               PROGRESS   DESCRIPTION
5f2a...   running  S-1-5-21-...-1001      S-1-5-21-...-1001      3/5 items  nightly backup
$ svctl job status 5f2a...
$ svctl job stop 5f2a...
$ svctl job submit --description "nightly backup" --timeout 3600 -- /usr/bin/backup --full
$ svctl --wait job submit --env MODE=full /usr/bin/backup   # exits 0 only if the job completed
$ svctl job wait --for ready 5f2a...
$ svctl job signal 5f2a... SIGHUP
```

`job submit` takes the definition fields as options: `--description TEXT`,
`--cwd DIR`, `--env NAME=VALUE` (repeatable), `--timeout SECS`,
`--stop-timeout SECS`, `--readiness none|notify`, `--readiness-timeout SECS`
, `--success-exit-code N` (repeatable), `--security-descriptor SDDL`. Two
hand the job something of yours: `--fd NAME=FD` passes a descriptor of the
`svctl` process to the job under `NAME` (from descriptor 3, with `LISTEN_FDS`
/`LISTEN_FDNAMES` set), and `--output` attaches `svctl`'s standard output as
the job's output sink, so the job's lines appear on your terminal as well as
in eventd. Everything after `IMAGE` belongs to the job, options included; use
`--` before an image path that starts with a dash.

The CLI has no way to attach a token, so a `svctl` -submitted job always runs
as the caller. Running a job as *someone else* is a programmatic act — a
service attaching the token of the client it is impersonating — and is
described in [Jobs and
operations](~peios/services-and-jobs/jobs-and-operations).

## The command × state matrix

A command sent to a service in an unexpected state returns an **error**, not a
silent no-op. This matrix is the authority on what each command does in each
[state](~peios/services-and-jobs/the-service-lifecycle):

| Command | Inactive | Starting | Active | Reloading | Stopping | Completed | Backoff | Failed | Abandoned | Skipped |
|---|---|---|---|---|---|---|---|---|---|---|
| **start** | Start | MERGE | ALREADY | ALREADY | QUEUE | Start | DEFER | Start | ERROR | Start |
| **stop** | NOOP | Cancel+Stop | Stop | Stop | MERGE | Clear | Cancel | NOOP | ERROR | NOOP |
| **restart** | Start | QUEUE | Restart | Restart | QUEUE | Start | Restart | Start | ERROR | Start |
| **reload** | ERROR | ERROR | Reload | MERGE | ERROR | ERROR | ERROR | ERROR | ERROR | ERROR |
| **reset** | NOOP | ERROR | ERROR | ERROR | ERROR | ERROR | ERROR | Clear | Clear | Clear |
| **status** | OK | OK | OK | OK | OK | OK | OK | OK | OK | OK |

Legend:

- **MERGE** — an operation of this type is already running; your command
  merges into it and you get its GUID.
- **DEFER** — your `start` is accepted and creates a Pending start operation,
  but it does *not* run yet: it waits out the service's existing backoff
  deadline and then starts. If a deferred start is already waiting, you merge
  into it and get its GUID.
- **ALREADY** — already in the target state; returns the current status, not
  an error.
- **QUEUE** — queued as Pending; runs after the current operation finishes.
- **NOOP** — no effect; returns the current status.
- **Clear** / **Cancel** — clear the state to Inactive / abort the current
  operation, then proceed.
- **ERROR** — invalid for this state; returns an error with an explanation.

The [Backoff](~peios/services-and-jobs/the-service-lifecycle) column is the
subtle one: the service is down with an automatic restart pending, so `start`
is *deferred* — it creates a Pending start that honors the remaining backoff
delay and only runs once that deadline expires (a second `start` merges into
the one already waiting), `stop` cancels the pending restart and any deferred
start (the service goes Inactive), `restart` cancels the automatic one and
does an admin restart, and `reload` /`reset` are invalid because no process
exists.

## Error codes

An error response is `{"status": "error", "code": "...", "message": "..."}`.
The `code` is one of:

| Code | Meaning |
|---|---|
| `ACCESS_DENIED` | AccessCheck denied the command against the target descriptor. |
| `UNKNOWN_SERVICE` | No such service definition (also returned for `start`/`restart`/`reload` on a [definition-removed](~peios/services-and-jobs/defining-a-service) service). |
| `UNKNOWN_OPERATION` | No such operation GUID — never existed, or dropped after its retention grace. |
| `UNKNOWN_JOB` | No such job GUID — never existed, or dropped after its retention grace. A known job you cannot query returns `ACCESS_DENIED`. |
| `MALFORMED_REQUEST` | The request line is not a single valid JSON object. |
| `REQUEST_TOO_LARGE` | The request exceeds `MaxRequestSize`. |
| `INVALID_COMMAND` | The `command` field is missing or unknown. |
| `INVALID_ARGUMENTS` | A required field is missing or malformed (for example `shutdown` with no valid `type`). |
| `INVALID_STATE` | Not valid for the service's current state (an `ERROR` cell above), or rejected because the system is already shutting down. |
| `OPERATION_TIMEOUT` | A `--wait` operation did not reach a terminal state within its timeout. |
| `INTERNAL_ERROR` | peinit hit an internal failure executing the command. |

The jobs socket uses the same envelope and codes, plus two of its own:

| Code | Meaning |
|---|---|
| `QUOTA_EXCEEDED` | The submission would exceed `MaxJobsPerSubmitter` live jobs for your SID. Nothing was created. |
| `BAD_TOKEN` | The token attached to a submission cannot be a job identity — more than one was attached, its impersonation level is below Impersonation, or it could not be duplicated. |

The `message` is human-readable and non-normative — read it for context, key
off the `code`.

## Exit status

| Code | Meaning |
|---|---|
| `0` | The command succeeded. For `job submit --wait`, the job also `completed`. |
| `1` | peinit refused the command — an access denial, unknown service or job, invalid state, or timeout — or a `job submit --wait` job ended other than `completed`. |
| `64` | A usage error. |
| `69` | peinit could not be reached — the socket is missing or refused the connection. |
| `70` | A protocol error — peinit answered with something the client could not read. |

## How the control interface works

Both tools use `/run/services/peinit/control.sock`. Connection admission and
each service action are access-controlled. The default socket admits signed-in
users; the target's permissions decide which commands they can run.

Lifecycle commands create an operation ID you can follow. Concurrent identical
requests can merge into one operation. For JSON framing and complete response
examples, see the [control-client
reference](~peios/advanced-peios/peinit/the-control-interface/operator-client-examples#how-the-control-interface-works).

## Connection limits

peinit enforces hard limits on the control socket, all tunable under
`Machine\System\Init\`:

| Key | Default | Limits |
|---|---|---|
| `MaxControlConnectionsPerUser` | 16 | Concurrent client connections one user can hold; SYSTEM is exempt (excess are refused at connect). |
| `MaxControlConnections` | 256 | Concurrent client connections in all (excess are refused at connect). |
| `MaxRequestSize` | 65536 | Bytes per request. |
| `ConnectionTimeout` | 30 | Seconds an *idle* connection can sit before it is closed. |

The jobs socket has its own set, under the same key:

| Key | Default | Limits |
|---|---|---|
| `MaxJobsConnections` | 64 | Concurrent jobs-socket connections (excess are closed at connect, with no response). |
| `MaxJobMessageSize` | 32768 | Bytes per message — which bounds a submission's whole definition. |
| `JobsConnectionTimeout` | 30 | Seconds an *idle* jobs connection can sit before it is closed. A connection blocked on a `wait` or a pending `submit` is not idle. |
| `MaxJobsPerSubmitter` | 64 | Live jobs one submitting SID can hold. SYSTEM is exempt. |

## Where to start

To understand the states the matrix refers to, read [The service
lifecycle](~peios/services-and-jobs/the-service-lifecycle).

To understand the operations every lifecycle command creates and how to poll
them, read [Jobs and
operations](~peios/services-and-jobs/jobs-and-operations).

To configure who can run each command, read [Who can manage a
service](~peios/services-and-jobs/who-can-manage-a-service).
