---
title: Finalization
description: The three shutdown paths that reach the kernel and how much work each does on the way, including what survives a failure.
---

Three shutdown paths reach the kernel, and they do different amounts of
work on the way. [*final.the-three-paths-do-different-amounts-of-work]

| Path | Stop waves | Seed save | Unmount | Sync | Final action |
|---|---|---|---|---|---|
| Graceful | Yes | Yes | Yes | Yes | Yes |
| Forced — three SIGINTs | No, SIGKILL everything | No | No | Yes | Yes |
| Critical service failure | No | No | No | Yes | Yes |

The two abrupt paths are required to reach `sync()` and the final kernel
action with minimal additional work, and skipping the seed and the
unmounts is what "minimal" means.
[*final.the-abrupt-paths-skip-the-seed-and-the-unmounts] A machine that
is rebooting because
its audit daemon died has nothing to gain from a tidy unmount and
something to lose from the time it takes.

## What survives a failure

Steps 6 and 7 — the seed and the unmounts — retain their failures as
evidence rather than acting on them. Every one is recorded, none of them
blocks step 8, and none of them enters recovery mode.
[*final.a-cleanup-failure-never-blocks-the-final-action] By this point
there is nothing to recover *to*: services have stopped and the
filesystems are on their way down.

## If the final action returns

`reboot(2)` does not return on success. If it does, the final action
failed, and peinit enters a minimal failed-shutdown state:
[*final.a-returning-final-action-enters-a-failed-shutdown-state]

- PID 1 stays alive. It has to — PID 1 exiting panics the kernel.
- The failure is recorded.
- The same action is retried, no more than once a second.
  [*final.the-failed-action-is-retried-at-most-once-a-second]
- Services are not restarted, and recovery mode is not entered.
  [*final.a-failed-final-action-does-not-restart-services-or-enter-recovery]

There is no way back from here. Finalization has begun, the services are
gone and the filesystems are read-only; the only correct behavior is to
keep trying the one thing that would end it.

`RB_HALT_SYSTEM` does not return either, so this state is only reachable
for a genuine failure rather than for the halt case.
[*final.rb-halt-system-does-not-return-either]

## A note on the mount table

The unmount step re-reads `/proc/self/mountinfo` when checking whether a
mount point that returned `ENOENT` or `EINVAL` is really gone. Depth
ordering puts the depth-one mounts last, alphabetically — `/dev`,
`/proc`, `/run`, `/sys` — so `/proc` is unmounted before `/run` and
`/sys` are attempted. [*final.proc-is-unmounted-before-run-and-sys]
Neither of those answers `ENOENT` or `EINVAL` in practice — `/sys`
unmounts, and `/run` either unmounts or is busy — so the check never
runs for them. It is a latent hazard rather than a live one: if either
ever did answer that way, the check could not read the file it needs,
and the shutdown would record a cleanup failure and attempt a pointless
read-only remount.
