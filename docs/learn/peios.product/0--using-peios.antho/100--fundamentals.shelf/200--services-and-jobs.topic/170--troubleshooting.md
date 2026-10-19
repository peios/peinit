---
title: Troubleshoot a Service
type: how-to
description: Start with status and logs, then diagnose start failures, retries, missing changes, access errors and boot recovery.
related:
  - peios/services-and-jobs/the-service-lifecycle
  - peios/services-and-jobs/supervision
  - peios/services-and-jobs/controlling-services
  - peios/services-and-jobs/boot-and-boot-modes
---

Collect the current state and cause before changing anything:

```sh
svctl status <service>
svctl list
svctl boot
```

Keep the job or operation ID and any warnings. Open **Logs…** for the service
in Services Manager to inspect its main process, hooks, and health checks. For
older attempts, query [logs and
events](~peios/services-and-jobs/output-and-logging); live status is not a
permanent history.

Use the symptom below to choose the next check. Avoid repeating a restart or
changing permissions until you know which layer failed.

## A service won't start

`status` shows it `Failed` or `Skipped`. The **cause** says why:

| Cause | What happened | What to do |
|---|---|---|
| `ValidationError` | The definition is malformed — a bad field, an unresolvable conflict, an illegal health-check timing. | Check the logs for the specific field. Fix the definition; the message names what is wrong. |
| `DependencyFailure` | A `Requires`/`BindsTo` target failed or does not exist. | Fix the *target* first — `status` it. The dependent recovers once the target can start. |
| `ConditionSkipped` (→ Skipped) | A start-time [condition](~peios/services-and-jobs/execution-environment) was not met. **Not a failure** — the service decided it does not apply here. | If it *should* run, the condition's premise is false (a missing path/file/key). This is often correct behavior. |
| `TtyUnavailable` (→ Skipped) | Another service is holding the terminal this one names in `TTYPath`. **Not a failure** — it is waiting its turn. | Check who has it: the holder is the other service with that `TTYPath` in a running state. Give this one a higher `TTYPrecedence` to win next boot, or a [`tty:released`](~peios/services-and-jobs/triggers-and-timers) trigger so it starts when the terminal frees. |
| `AssertionError` | A start-time [assert](~peios/services-and-jobs/execution-environment) failed — a required precondition is missing. | Create the missing precondition (path, file, directory, registry key), then start again. |
| `PreHookFailure` | An `ExecStartPre` hook exited non-zero. | Inspect the hook's captured output and its [identity](~peios/services-and-jobs/identity-and-privileges). Before rerunning it manually, check for side effects: setup and migration hooks can change data. |
| `ParentSetupFailure` | peinit could not even fork — fd/PID exhaustion, a cgroup error. No process was created. | A system-resource problem, not the service's fault. Check for fd/PID limits and cgroup health. |
| `PreExecFailure` | Setup after fork failed — token install, rlimits, environment. | Usually identity: check `Identity`, that authd is up, and that `RequiredPrivileges` names real privileges. |
| `ReadinessTimeout` | The process started but never became [ready](~peios/services-and-jobs/service-types) within `StartTimeout`. | Either the service is genuinely slow (raise `StartTimeout`, or have it send `EXTEND_TIMEOUT_USEC`) or it never sends `READY=1` (wrong `Readiness`, or a bug). |

> [!TIP]
> A startup failure (`ReadinessTimeout`, `PreHookFailure`, `PreExecFailure`,
> `ParentSetupFailure`) is **restart-eligible**. Whether it was retried
> depends on the configured policy and budget; `Failed` alone does not prove
> multiple attempts occurred. Read the [restart
> policy](~peios/services-and-jobs/supervision) and the logs for the actual
> attempts.

## A service keeps restarting

The service flaps — up, down, up, down. This is [restart
policy](~peios/services-and-jobs/supervision) doing its job, but it points at
an unstable service.

- **It eventually settles in `Failed` with `RestartBudgetExhausted`.** It
  crashed `RestartMaxRetries` times faster than it could stay healthy for
  `RestartWindow`. The fix is the *service*, not the policy — read its logs
  for the crash. Raising the budget only delays the inevitable.
- **It flaps forever and never exhausts the budget.** Check whether it stays
  Active for the full `RestartWindow` between failures. Reaching Active
  briefly does not reset the budget. Inspect the
  [health-check](~peios/services-and-jobs/supervision) output too; a faulty
  check can restart a working service.
- **It restarts on a clean exit you did not expect.** `RestartPolicy=Always`
  restarts even successful exits (cause `CleanExitRestart`). If the service
  is *meant* to exit, use `OnFailure` instead.

## The machine keeps rebooting

Preserve the last console error and any service records before attributing
repeated reboots to a service. **If peinit reports a Critical service
exhausting its restart budget**, its
[`ErrorControl=Critical`](~peios/services-and-jobs/supervision) policy makes
peinit sync and reboot. Repeating that failure can produce a reboot loop; the
symptom alone does not establish this cause.

The [boot-attempt counter](~peios/services-and-jobs/boot-and-boot-modes) can
break a sequence of boots that reach peinit's counter increment but never
become confirmed: after N attempts (default 3), peinit enters [Recovery
mode](#recovery). If each boot reaches the success grace
before the Critical failure, its counter is reset and the reboot loop can
continue without automatic Recovery. Check `svctl boot` while the normal
control interface is available; it is unavailable in Recovery. Do not assume
every reboot loop will stop after three attempts, or that peinit can deliver
Recovery if it cannot start.

In Recovery, start with the failure reason on the console. Phase 2 services
are skipped, so stored eventd history is not automatically queryable from that
shell. Where a supported running eventd query service is available, use [Find
service logs](~peios/services-and-jobs/output-and-logging) and [Find missing
records](~peios/reading-records/finding-missing-records) to investigate earlier
attempts. `evctl` queries that service; it does not open stored databases
offline. Otherwise, retain the console evidence for the administrator or
provider's recovery procedure.

Once the evidence identifies a Critical service failure, repair its cause or
restore known-good configuration through a supported recovery path before
rebooting. Disabling a required platform service can leave its dependents
unable to start. If you need to intervene before the counter trips, boot with
`peios.recovery=1` on the kernel command line.

## Abandoned

`Abandoned` means peinit SIGKILLed the process but it survived — stuck in
uninterruptible kernel sleep (**D-state**), the signature of hung I/O (a dead
NFS mount, a failing disk). Nothing in userspace can kill a D-state process.

- The underlying **I/O fault is the real problem** — investigate the storage
  or mount, not peinit.
- The service's cgroup is **leaked** and shows in `warnings`. A later start
  uses a fresh [generational
  cgroup](~peios/services-and-jobs/execution-environment), so the new instance
  is unaffected.
- `svctl reset <service>` clears the Abandoned state and re-checks the cgroup:
  if the stuck process finally died, peinit cleans up; if not, it stays leaked
  and warns you. Leaked cgroups clear fully only on reboot.

## I changed the config and nothing happened

peinit works from an [in-memory
snapshot](~peios/services-and-jobs/defining-a-service), and each field has a
**mutability class** that decides when a change applies:

- **Immutable at runtime** (`ImagePath`, `Type`, `Identity`,
  `RequiredPrivileges`, `ErrorControl`) → takes effect only on `restart`.
- **Apply on next start** (dependencies, conditions, `OnFailure`) → next
  start or graph reload.
- **Reloadable at runtime** → next relevant operation. Timeouts and
  health-check policy can affect later checks; environment, arguments, working
  directory and process limits apply to a new process, not one already
  running.
- **Hot-reloaded** (`ServiceSecurity`) → next control request.

If a change is not landing, check its class first. For a wholesale re-read of
every definition, run `svctl reload-config` — it rebuilds and validates the
whole graph atomically and swaps it in only if validation passes (running
services are untouched). And remember peinit *pulls* changes from registry
notifications rather than having them
pushed, so there can be a brief lag.

## ACCESS_DENIED

A control command returns `ACCESS_DENIED`. peinit ran
[AccessCheck](~kernel/internals/kacs/access-check/the-dacl-walk) on your token against the
target's descriptor and the right was not granted:

- A *service* command needs the matching right in the service's
  [ServiceSecurity
  descriptor](~peios/services-and-jobs/who-can-manage-a-service) (`start`
  needs `SERVICE_START`, `restart` needs both stop and start, …).
- `shutdown` and `reload-config` need `SYSTEM_SHUTDOWN` /
  `SYSTEM_RELOAD_CONFIG` in peinit's control descriptor.
- The check uses your **effective** identity at connect time — if you are
  [impersonating](~kernel/internals/kacs/impersonation/overview), that is what is checked.

Under the default descriptors every denial is recorded by the kernel as a
`kacs.audit.access.checked` event naming the caller, the target (its **object
kind** is `service`, `job`, or `peinit-system`) and the rights requested
and granted. Walk it through Debugging a
denial.

> [!NOTE]
> If `list` shows fewer services than you expect, that is not a bug — `list`
> **omits** services you lack `SERVICE_QUERY_STATUS` on rather than denying
> them. You are seeing exactly what your token can see. `job list` does the
> same with `JOB_QUERY`.

## A job submission is refused

A `job submit` — from `svctl` or from a service — comes back with an error
rather than a job. The code says which door it hit:

| Code | What happened | What to do |
|---|---|---|
| connection refused, or an immediate close | You cannot reach `/run/services/peinit/jobs.sock` — its file descriptor does not grant you write, or peinit is at `MaxJobsConnections`. peinit itself performs no check on who can submit; the socket's descriptor is the whole policy. | Check the socket's descriptor (by default Authenticated Users can connect) and the connection count. |
| `QUOTA_EXCEEDED` | Your SID already holds `MaxJobsPerSubmitter` live jobs (default 64). | `job list --submitter <your SID>` to find them; stop what should not be running, or raise the key. SYSTEM is exempt. |
| `BAD_TOKEN` | The token you attached cannot become a job identity: more than one was attached, it is below Impersonation level, or peinit could not duplicate it. | Attach exactly one token obtained by impersonating the client; an Identification-level token is never enough. If the job needs Delegation, set the jobs connection to Delegation *before* attaching. |
| `INVALID_ARGUMENTS` | The definition is malformed — a relative `image_path`, a `descriptors` list that does not match what you attached, a `security_descriptor` without an owner and DACL. | The message names the field. |
| `INVALID_STATE` | The system is shutting down; submissions are refused. | — |

A job that was *accepted* but never ran is not an error response: it is an
`ok` response whose view is `failed` with a `cause` — `parent_setup_failure`
(peinit could not prepare the launch) or `pre_exec_failure` (the program could
not be executed as that identity, which is where "no such file" and
"permission denied" land, because peinit does not check the image before
accepting a submission).

## A job's output has gaps

If a submitter attached an output sink and sees gaps, look for a
`peinit.job.output.dropped` event for the job in
[eventd](~kernel/internals/kacs/auditing/overview): the sink was not being drained fast
enough, and peinit dropped lines *for the sink only* rather than let it slow
the job. Query eventd under the job's GUID for the independently forwarded
record. Sink loss does not itself drop that record, but eventd's log path is
also best-effort, so completeness is not guaranteed.

## UNKNOWN_JOB

The GUID is unknown or its retention has elapsed. A submitted job is retained
for 60 seconds after it ends, then dropped; check eventd for any stored
record. A known job that you lack permission to query returns `ACCESS_DENIED`
. `job list` instead omits jobs you cannot query.

## A dependent never started

You started (or booted) a service, but something that depends on it never came
up:

- A **`Requires`/`BindsTo` dependent stays blocked** until its target reaches
  a [satisfying state](~peios/services-and-jobs/the-service-lifecycle)
  (Active, Reloading, Completed, or Skipped). If the target is stuck in
  Starting or Failed, so is the dependent — fix the target.
- If a dependent got *connection refused* on boot, check the target's
  readiness setting and startup logs. With **`Readiness=Alive`**, peinit
  waited only for the process to exist, not to be serving. Use
  `Readiness=Notify` only if the program supports sending `READY=1` from its
  tracked main process when ready; changing the setting does not add that
  support and can cause `ReadinessTimeout`. See the [notification
  contract](~peios/services-and-jobs/service-types#the-sd-notify-contract). If
  the program cannot signal readiness, have its service or package owner
  resolve the startup race. peinit's boot validation warning can identify an
  Alive-readiness dependency; it does not prove the cause of a refused
  connection.
- A **`Wants` dependent waits for the optional target's startup attempt to
  resolve**, but starts even if the target failed. Use `Requires` when
  successful readiness is necessary.

## Safe

[Safe mode](~peios/services-and-jobs/boot-and-boot-modes) selects Critical and
`SafeMode=1` services within the boot-triggered set. Those flags do not give a
demand-only service a boot trigger. You land here when graph validation finds
a **cycle or unresolvable conflict involving a Critical service**, or when
`peios.safemode=1` is on the kernel command line. The TCB is healthy; the
*configuration* is broken.

Fix the graph: the logs name the cycle path (`A → B → C → A`) or the
conflicting pair. Once the cycle is broken or the conflict resolved, a normal
boot returns. You can start any service by hand from Safe mode in the meantime
— it is only auto-start that is restricted.

## Recovery

[Recovery mode](~peios/services-and-jobs/boot-and-boot-modes) gives you a
SYSTEM shell on the console and skips Phase 2 services. It starts registryd
only if the boot has not already attempted that start. You reach it when the
boot counter hits N, when `peios.recovery=1` is set, or when **registryd
failed in Phase 1** (Recovery is immediate; this boot attempt has already
incremented the counter).

If the registry itself is failing:

1. Preserve peinit's console failure reason and the source's startup
   diagnostics. A failed registry operation does not prove the source process
   has exited.
2. Identify the installed `registryd` provider and version, its hive
   declarations and actual database paths. [LCS and
   sources](~peios/registry-sources/overview) explains the
   default `loregd` provider and its diagnostics.
3. Establish whether the source is already active before any storage work.
   Recovery can retain a registryd started or attempted earlier in the boot.
   **Do not launch a second source against the same hive files.**
4. Obtain the installed provider's supported recovery procedure before
   attempting offline inspection or repair. The [source-checked loregd
implementation](~peios/loregd/startup/command-line#unsupported-recovery-switches)
   accepts hive declarations and has no offline repair switches. It [does not
   create an automatic startup
   backup](~peios/loregd/startup/startup-sequence#backup-and-repair-limits);
   its orphan cleanup is not corruption repair or backup restoration. Those
   references identify the checked revision. Verify what recovery copy
   actually exists and what it contains before planning a replacement.

When the source is serving the required registry operations, [Backup and
restore](~kernel/internals/lcs/backup-and-restore/overview) describes the
supported privileged subtree workflow. That workflow depends on a working
source; it does not repair an unavailable source. Restore replaces the target
contents and descendants, including security descriptors, so review the target
and backup before using it. Role definitions can re-supply service
configuration but do not restore every registry value.

Recovery offers **no TCB guarantee**. Treat the unrestricted SYSTEM shell
accordingly, and remember it needs console access (physical, IPMI, or serial);
there is no remote recovery yet.

## Where to start

To read states and causes fluently, keep [The service
lifecycle](~peios/services-and-jobs/the-service-lifecycle) handy.

For restart, backoff, and Critical-reboot behavior, see [Keeping services
running](~peios/services-and-jobs/supervision).

For the commands and their outputs, see [Controlling
services](~peios/services-and-jobs/controlling-services).
