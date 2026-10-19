---
title: Leaked Sub-Cgroups
description: A D-state process does not die when SIGKILLed — what happens to its cgroup, and how the leak becomes visible.
---

A process in uninterruptible kernel sleep — D-state, typically a hung
NFS mount or a failing disk controller — does not die when it is
SIGKILLed. Its cgroup cannot be removed while it is there.

peinit detects this through `cgroup.events`: after sending the kill it
arms a post-kill deadline, 5 seconds by default, and checks whether
`populated` is still 1 when the deadline fires.
[*cgroup.a-post-kill-deadline-detects-a-survivor]

## What happens depends on which cgroup it is

For the **main process**, a survivor is fatal to supervision: the
service transitions to Abandoned with cause `ProcessUnkillable` and
peinit stops supervising it (§6.1).
[*cgroup.an-unkillable-main-process-abandons-the-service]

For a **health check or a hook**, it is not. Those are diagnostic and
setup processes; they hold no service resources — no ports, no file
locks, no database connections — so a stuck one does not make the
service unmanageable. peinit orphans the sub-cgroup instead:
[*cgroup.a-leaked-hook-or-health-cgroup-is-orphaned]

1. Marks it leaked, recording the path, the kind, and the time.
2. Increments the service's cgroup generation, so the next start builds
   a fresh tree (§5.1).
3. Carries on supervising the service normally.

A leaked **pre-start check helper** is treated the same way: its
`checks/` sub-cgroup is recorded and the generation bumped, which matters
because otherwise the next start would build into a tree that still
contains the unkillable process.
[*cgroup.a-leaked-check-helper-is-treated-the-same-way]

The leaked cgroup stays in the hierarchy until the next reboot.

A tree the service's next instance is already running in is not
checked. A restart can relaunch before the post-kill deadline of the
instance it replaced — the default restart delay is one second, the
post-kill deadline five — and it relaunches into the same tree, since
nothing has leaked yet to move the generation on. When the deadline
fires, the tree is populated by the live instance, not by a survivor,
so peinit drops the check rather than record a leak.
[*cgroup.a-tree-back-in-use-is-not-a-leak] A survivor of the killed
instance that shares that tree goes unreported until the tree is next
killed and checked.

## Visibility [*cgroup.a-leak-is-both-pushed-and-queryable]

Leaks are not silent. peinit both pushes one when it is detected and
keeps it queryable afterward.

The push happens once, on first detection — recording is idempotent, so
a leak that is re-examined on a later cleanup pass is not announced
again: [*cgroup.a-leak-is-announced-once]

- A **`peinit.cgroup.leaked` event** on the event stream, carrying the
  service, the sub-cgroup path (`object.cgroup.path`) and its kind
  (`object.cgroup.type`: `service-tree`, `health`, `hooks`, or `helper`).
  When it was found is the event's own time. [*cgroup.the-leak-event]

- A **console line** naming the same service, kind and path.
  [*cgroup.the-leak-console-line]

The pull side survives the moment of detection, for anyone who was not
watching:

- A **status query** includes a `warnings` array, one entry per leak,
  each an object with the sub-cgroup path, its kind, and the time of
  detection: [*cgroup.status-carries-a-warnings-entry-per-leak]

  ```json
  {"path": "/sys/fs/cgroup/peinit/jellyfin/health", "type": "health",
  "detected_at": "2026-06-01T12:34:56.123456789Z"}
  ```

- A **start command** on a service with leaks returns a warning in its
  acknowledgment, saying the service has leaked sub-cgroups from a
  previous generation and that this indicates an I/O problem needing
  investigation. [*cgroup.a-start-on-a-service-with-leaks-warns]

The `type` is the same vocabulary everywhere
[*cgroup.the-leak-type-vocabulary]: `health` for a leaked
`health/` sub-cgroup, `hooks` for a leaked `hooks/` one, `helper` for a
pre-start check helper's `checks/`, and `service_tree` for a leaked
service root — the last being the most serious, since it means the whole
tree including `main/` could not be reclaimed.

Whichever way it reaches you, what the leak means is the same: something
underneath the service is not responding to the kernel, and no amount of
restarting the service will fix it.
