---
title: The Graceful Sequence
description: The ordered steps of a graceful shutdown, from entering the shutdown state to the final power action.
---

## Step 1: Enter the shutdown state

peinit sets an internal flag. While it is set, no new service starts.
[*graceful.no-new-service-starts-once-the-flag-is-set] Control commands
other than `status`, `list`, `operation-status`, `job-status`,
`job-list`, and `job-stop` are rejected as invalid for the current
state.

Every live submitted job (§8.5) is stopped at this point too: SIGTERM
to each — unless it has already sent `STOPPING=1` — with cause
`shutdown`, its own `stop_timeout` to the kill, and no ordering between
jobs or against the service waves, because a job has no dependencies
to order by. [*graceful.every-live-submitted-job-is-stopped-at-once]
A job still queued for launch is canceled with the same
<!-- lint-disable-next-line spelling -->
cause. [*graceful.a-queued-job-is-cancelled-with-the-same-cause]
`submit` is refused for the rest of the shutdown; `status`,
`wait`, `stop` and `signal` keep answering, as do the control socket's
`job-status`, `job-list` and `job-stop`.
[*graceful.submit-is-refused-while-the-other-job-commands-keep-answering]

Timer triggers are disarmed. The timerfds stay armed and keep firing —
peinit gates the handler rather than the fd, matching how every other
event source is handled — but a firing during the shutdown window is
classified as a no-op and nothing is started or queued.
[*graceful.a-timer-firing-during-shutdown-is-a-no-op]

The gate matters more than the rule reads. The states a timer firing
would start from are Inactive, Completed and Failed, precisely the
states step 3 classifies as not participating, so a firing would create
a service after the plan was fixed: in no wave, not waited for, and not
reached by the global-timeout sweep, still holding files open when
peinit reaches the unmount step.

## Critical

A Critical service failing during shutdown is recorded but does not
trigger a reboot.
[*graceful.a-critical-failure-during-shutdown-does-not-reboot]
The system is already going down, and rebooting from
here would loop.

## Step 3: Classify

Completed services — Oneshots with `RemainAfterExit` — have no process
and are transitioned to Inactive, releasing the dependency
relationships they were holding so their dependents can be stopped
cleanly. [*graceful.completed-services-are-transitioned-to-inactive]

The rest are classified for stop eligibility:

- **Active and Reloading** are graceful-stop eligible and enter the
  waves. [*graceful.active-and-reloading-enter-the-waves]
- **Stopping** services are already on a stop path. They join the waves
  for ordering and timeout purposes, but do **not** receive another
  SIGTERM.
  [*graceful.a-stopping-service-joins-the-waves-without-another-sigterm]
- **Starting** services are not eligible. peinit cancels the startup,
  SIGKILLs the service cgroup if one exists, and transitions them to
  Failed with cause `ShutdownWave` while post-kill verification is
  pending. [*graceful.a-starting-service-is-killed-and-failed]
  A cgroup still populated after the post-kill timeout takes
  the service to Abandoned with cause `ProcessUnkillable` and the cgroup
  is leaked.
  [*graceful.a-cgroup-that-survives-the-post-kill-timeout-is-abandoned]
  A Starting service whose job never forked skips the check
  and goes straight to Failed.
  [*graceful.an-unforked-starting-job-goes-straight-to-failed]
- **Inactive, Failed, Skipped, Backoff, and Abandoned** do not
  participate. [*graceful.five-states-do-not-participate]
  Abandoned cgroups stay leaked and shutdown continues.

<!-- lint-disable-next-line line-length -->
## Step 4: Stop in reverse dependency order [*graceful.no-service-stops-until-its-dependents-have]

peinit builds the reverse dependency graph over hard dependencies and
stops in waves:
<!-- lint-disable-next-line line-length -->
[*graceful.each-eligible-service-gets-sigterm-then-a-cgroup-sigkill-at-stoptimeout]

1. Each eligible service receives SIGTERM. Already-stopping ones do not.
2. Each has `StopTimeout` to exit.
3. On expiry, SIGKILL to the service's entire cgroup.
4. No service is stopped until everything depending on it has stopped.

The compiled-in Phase 1 service, registryd, stops in a wave of its own
after every other participant, though no definition declares a
dependency on it. [*graceful.registryd-stops-after-every-other-service]
Every service defined in the registry rests on it, as it started before
any of them; stopped by the graph alone it would land in the first wave
and go while the services still running needed it. The rule follows the
definition's compiled-in provenance, not its name.

A participant the service table does not have counts as stopped.
[*graceful.a-participant-gone-from-the-table-counts-as-stopped] A
service whose definition is withdrawn while it runs is kept until it
stops and then discarded (first-boot setup withdraws its own service
this way), and the plan fixed at step 3 still names it. Waiting for it
would hold its wave open for good.

A service that sent `STOPPING=1` does not receive a SIGTERM at all: it
has already said it is shutting down, and peinit goes straight to the
stop deadline. [*graceful.a-service-that-sent-stopping-gets-no-sigterm]
The notification is acknowledged as a `peinit.notify.stopping.reported`
event (§10.5), which is what makes the suppressed SIGTERM legible
afterward rather than looking like one peinit failed to send.

### Observing the waves

Every stop-wave participant receives its own `Shutdown` -sourced `Stop`
record when the shutdown plan is fixed. Its requested event records the
intent; its started event records when the participant's wave becomes
eligible, including when an existing stop is adopted without another
SIGTERM. The records and their identifiers are visible through the
ordinary operation queries and `peinit.operation.*` events.
[*graceful.shutdown-stop-observations-follow-the-waves]

A service that exits cleanly before its wave completes its Pending
observation without a started event. A crash before the wave fails it.
For a service already stopping, reaching Inactive completes the
observation; a stop timeout, global timeout, or forced reboot fails it.
Post-kill abandonment does not produce a second terminal result for an
observation already failed at the kill. Starting services retain their
existing startup-failure records, and Completed services cleared without
a process do not receive stop observations.

An administrator Stop or Restart already in flight keeps its identity
and its original timing evidence. The shutdown observation is separate;
it neither supersedes that record nor queues a second control command.

### Timing an already-stopping service

peinit does not reset an already-Stopping service's clock, either when
shutdown begins or when its wave becomes eligible.
[*graceful.an-already-stopping-services-clock-is-not-reset] It uses the
timing evidence retained from the stop path that put the service in
Stopping:

- If a Stop operation, or a Restart executing its stop leg, is in
  flight, that operation's retained timing governs.
- Otherwise peinit uses service-level evidence, which carries the cause
  that initiated the transition — `ExplicitStop`, `ShutdownWave`,
  `ConflictEviction`, or `BindsToPropagation`.

peinit requires the evidence to be present, to belong to a service
actually in Stopping, to carry a cause matching the service's current
cause, to name one of those four causes, and not to describe a deadline
earlier than its own start.
[*graceful.the-retained-evidence-must-meet-five-conditions]

Evidence that is missing, stale, or ambiguous **fails closed** rather
than being guessed at. [*graceful.unsubstantiable-evidence-fails-closed]
A stop that began before the shutdown has already
spent part of its own clock, and inventing a fresh budget here would
hand the service a second full `StopTimeout`.

Failing closed costs that one participant, not the shutdown.
[*graceful.failing-closed-costs-only-that-participant] A service
whose evidence cannot substantiate a deadline is given no graceful
period at all: its deadline is already due, so the timeout scan
escalates it to SIGKILL on its wave, and the console says which way the
evidence was unusable:
[*graceful.an-unsubstantiated-deadline-kills-without-a-graceful-period]

```text
peinit: shutdown cannot substantiate a stop timeout for <service>
(<reason>); killing it without a graceful period
```

Every other participant keeps its full `StopTimeout`, and the shutdown
runs to completion.

An operation whose lifetime expires while it is still Pending — a
later-wave stop waiting for its dependencies — fails that operation and
its waiters. It does **not** authorize signaling the service before its
wave is eligible.
[*graceful.a-pending-operations-expiry-does-not-signal-the-service-early]
Shutdown owns the signaling; the operation object is
an observation of it.

<!-- lint-disable-next-line line-length -->
## Step 5: Global timeout [*graceful.shutdowntimeout-and-postkilltimeout-bound-the-sequence]

| Key | Default | Meaning |
|---|---|---|
| `Machine\System\Boot\ShutdownTimeout` | 90 | Seconds for the whole sequence. |
| `Machine\System\Boot\PostKillTimeout` | 5 | Seconds for a cgroup to drain after SIGKILL. |

On expiry, every remaining participant's cgroup is killed, anything that
does not drain within the post-kill timeout is marked Abandoned with its
cgroup leaked, and shutdown continues regardless.
[*graceful.on-the-global-timeout-everything-remaining-is-killed]
Live submitted jobs
are killed in the same sweep, whatever phase their stop had reached,
and abandoned on the same terms.
[*graceful.the-global-timeout-sweep-kills-live-jobs-too]

The global timeout fires once. [*graceful.the-global-timeout-fires-once]
What its sweep killed is then governed by the post-kill timeout alone:
sweeping again would kill the survivors again and push their post-kill
checks back each time, so nothing would ever be abandoned and the
sequence would never reach step 6.

The console says what the sequence was waiting for when the timeout
fired, and, after any later timeout turn that leaves it still waiting,
what remains: participants with their state and wave, pending stop and
post-kill checks, and live jobs.
[*graceful.the-console-names-what-the-global-timeout-waited-for]

```text
peinit: shutdown global timeout expired waiting for lpsd (Active, wave 1), job <id>
peinit: shutdown still waiting for post-kill check of lpsd
```

Submitted jobs are named on the console as they are stopped, killed and
abandoned: `peinit: shutdown stopping job <id>`, `killing job <id>`, and
`abandoned job <id>`.

The sequence does not move to step 6 while a submitted job is live.
[*graceful.step-6-waits-for-every-live-submitted-job] A
job's reap advances shutdown progress exactly as a service's does, so a
session that exits promptly on SIGTERM costs the shutdown nothing, and
one that does not costs it `stop_timeout` and the post-kill grace.

## Step 6: Save the random seed

After every service has stopped and before any unmount, peinit writes a
fresh seed to `/var/state/peinit/random-seed` for the next boot: 512
bytes from the kernel CSPRNG on this machine, never copied from an
image, protected so that only SYSTEM-equivalent authority can read or
replace it. [*graceful.a-512-byte-seed-is-written-before-any-unmount]

The write is crash-conscious: a temporary file on the same filesystem,
written, flushed, atomically renamed over the old seed, and the
containing directory flushed.
[*graceful.the-seed-write-is-a-flush-and-atomic-rename] A failure is
recorded and shutdown continues.

The forced and Critical-reboot paths skip it, along with the unmount
step, and go directly to sync and the final action.

## Step 7: Unmount [*graceful.every-non-root-mount-is-unmounted-deepest-first]

1. Snapshot the mount table first, from `/proc/self/mountinfo`.
2. Attempt to unmount every remaining non-root mount in the namespace —
   not only what peinit mounted, so the Phase 1 set is covered by
   construction.
3. Process in descending path depth, so children go before parents.
4. A mount point already gone is a successful no-op.
   [*graceful.a-mount-point-already-gone-is-a-no-op]
5. On failure, attempt a read-only remount. If that fails too, record it
   and continue.
   [*graceful.a-failed-unmount-falls-back-to-a-read-only-remount]
6. The root is never unmounted, but is remounted read-only at the end.
   [*graceful.the-root-is-remounted-read-only-never-unmounted] A
   failure there is recorded and does not stop step 8.

## Step 8: Sync and the final action

This step is irreversible. Cleanup failures retained from steps 6 and 7
affect diagnostics only and never block it.

1. `sync()`. Called on all three paths — graceful, forced, and
   Critical reboot. [*graceful.sync-is-called-on-all-three-paths]
2. `reboot(2)` with `RB_POWER_OFF`, `RB_AUTOBOOT`, or `RB_HALT_SYSTEM`.
   [*graceful.the-final-action-is-reboot2-with-the-kinds-command]
3. If `reboot(2)` returns, the final action failed. peinit enters a
   minimal failed-shutdown state, keeps PID 1 alive, records the
   failure, and retries the same action no more than once a second. It
   does not restart services and does not enter recovery mode —
   finalization has begun and there is nothing to go back to.

## Shutdown during boot

A shutdown requested while Phase 2 is still running takes effect
immediately, through the same classification: Starting services are
SIGKILLed, services that reached Active are stopped gracefully, and the
boot is abandoned.
[*graceful.a-shutdown-during-phase-2-takes-effect-immediately]
