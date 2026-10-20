---
title: Service Removal
description: What happens when a definition disappears from the registry, depending on whether the service is running.
---

When a definition disappears from `Machine\System\Services\`, peinit
learns of it through the ordinary change-notification path.
[*remove.a-removal-arrives-through-the-watch] What happens next depends
on whether anything is running.

## Not running

An entry in Inactive, Failed, Completed, Skipped, Abandoned, or **Backoff**
is discarded immediately.
[*remove.a-non-running-entry-is-discarded-immediately]
There is no process to consider.

Backoff belongs in this list rather than the next one,
[*remove.backoff-is-discarded-like-any-other-non-running-state] which is
not obvious: a service between restart attempts looks like it has
something pending. It does not. It has no process, and once the definition is
gone
there is nothing left to restart it from, so there is neither an instance
to supervise nor a restart to wait for.

## Running

An entry in Active, Starting, Reloading, or Stopping is **not**
killed. [*remove.a-running-instance-is-not-killed] The running process
is a job, and a job's lifecycle is independent of the definition that
produced it — removing a definition stops future management, it does not
terminate work in progress.

peinit marks the entry **definition-removed** and keeps the cached
definition, solely to go on supervising the instance it already has.
[*remove.the-entry-is-marked-definition-removed] When that instance
exits it is not restarted — `RestartPolicy` is moot, because there is
nothing left to restart from — and peinit then discards the entry.
[*remove.an-exit-is-not-restarted-and-the-entry-is-discarded]

A definition-removed entry never has dependents. A read in which one
service `Requires` another the read does not define is refused whole,
with `MissingHardDependency` — so a removal that would leave a dangling
hard dependency does not land at all, and the administrator has to
remove the dependents first.
[*remove.a-removal-leaving-a-dangling-dependency-is-refused]

While an entry is definition-removed:

- `stop` is accepted, so an administrator can drain it cleanly, using
  the cached `StopTimeout`;
- `start`, `restart`, and `reload` are rejected with `UNKNOWN_SERVICE` —
  there is no definition to work from;
- `status` reports the current runtime state with `definition_removed`
  set, so the draining instance is visible rather than silent.

`definition-removed` is a flag on the existing runtime state, not a
state of its own. The state machine (§6.1) and the command × state
matrix (§10.3) are unchanged by it.

A `restart` already in its stop phase is a case of its own. The stop
phase still drains the existing instance; the start phase does not
begin, because there is nothing to start from. When the instance exits,
the operation is aborted with the reason
`definition_removed_during_restart_stop_leg` and the entry takes the
ordinary removal discard. [*remove.a-restart-in-its-stop-leg-is-aborted]
This is the window a package upgrade opens —
remove the definition, then restart the service.

Once the instance exits or is stopped, the entry — including anything in
its fd store (§10.6) — is discarded. A crash counts: the exit is routed
to Failed rather than into a restart, because there is no policy left to
apply. [*remove.a-crash-is-routed-to-failed-rather-than-a-restart] The
fd store goes with it, which a crash would otherwise preserve for the
restart that is not going to happen.
