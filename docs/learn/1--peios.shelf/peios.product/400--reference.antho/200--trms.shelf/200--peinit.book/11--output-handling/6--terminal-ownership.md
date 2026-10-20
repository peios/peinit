---
title: Terminal Ownership
description: A TTYPath has one owner at a time — how peinit picks it, what happens to the losers, and how a terminal is handed on when it comes free.
---

A `TTYPath` (§3.2) names a device that has exactly one owner at a time.
[*terminal.a-terminal-has-exactly-one-owner-at-a-time]
Two services writing to the same terminal do not fail; they interleave.
On a console that produces a sign-in prompt with a progress line through
the middle of it, and a password echoed into whatever redrew last. So
peinit hands the device to one service and makes the others wait.

<!-- lint-disable-next-line line-length spelling -->
## Who is holding it [*terminal.a-service-holds-its-terminal-while-it-may-have-a-process-on-it]

A service holds its terminal in every state where it can still have a
process on the device:

| Holding | Released |
|---|---|
| Starting, Active, Reloading, Stopping, **Backoff** | Inactive, Completed, Failed, Skipped, **Abandoned** |

The two emphasized entries are the ones worth stating outright.

**Backoff holds.** [*terminal.backoff-holds] A service between restart
attempts is coming back,
and handing its console to somebody else during the gap produces two
owners a second later. This is deliberately *not* the "settled" rule
used by `boot:settled` (§2.5), which also treats Backoff as still
moving, but reaches it from the opposite direction.

**Abandoned releases** [*terminal.abandoned-releases], and that one is a
compromise. The process
survived SIGKILL (§5.7) and can well still be writing. But peinit has no
remaining way to reclaim the device from it, so treating it as held
would mean the terminal is lost for the rest of the boot — and a console
nobody can sign in on is worse than a console with a stuck process on it.

Nothing is stored. The holder is derived from the service table, so
there is no claim to leak, to double-release, or to fall out of step
with the state machine.

<!-- lint-disable-next-line line-length -->
## Losing the terminal [*terminal.every-start-path-asks-first-whether-the-terminal-is-free]

Every start path — boot, deferred, on-demand, restart, and the
completion leg after a filesystem check helper (§5.2) — asks whether the
terminal is free before it asks anything else. If another service holds
it, the start ends there: the service goes to **Skipped** with cause
`TtyUnavailable` (§6.3), its operation completes, and its dependents are
satisfied exactly as any other skip satisfies them.
[*terminal.a-held-terminal-skips-the-start-with-ttyunavailable]

Skipped rather than Failed, because nothing is wrong. Nothing is written
to the console about it either [*terminal.a-terminal-skip-is-not-announced]
— it is the mechanism working, and the
message would land on the very terminal whose new owner is at that
moment drawing on it.

The check runs ahead of the service's own conditions and asserts, which
is why a service queued on a busy terminal never forks a check helper.
[*terminal.the-terminal-check-precedes-conditions-and-asserts]

The graph paths — the Phase 2 boot plan, and any start that brings
dependencies with it — ask twice. Their pre-start check runs for every
service released together before any of them has moved, so two
boot-plan services naming one terminal both pass it. The question is
then asked again at the moment each one would enter Starting, and by
then the first holds the device: the second is skipped there, with the
same cause and the same completed operation, rather than launched into
a terminal that already has a session.

A relaunch from Backoff asks too, and a service whose terminal was taken
while it waited goes from Backoff to Skipped (§6.2).

## Winning the terminal [*terminal.higher-tty-precedence-wins]

`TTYPrecedence` decides who wins when several services want the same
device at the same moment. Higher wins; equal precedence breaks on
service name [*terminal.equal-precedence-breaks-on-service-name], so an
operator who states no preference still gets the
same machine on every boot rather than whichever definition the registry
happened to enumerate first. The default is 0.
[*terminal.the-default-precedence-is-zero]

Precedence is consulted among *simultaneous* candidates only. A service
that already holds a terminal is never preempted by a higher-precedence
latecomer [*terminal.a-holder-is-never-preempted]: taking a live
terminal away from a process that is using it
would lose whatever the operator was in the middle of typing.

Two places have simultaneous candidates. Deferred starts (§2.5) are
dispatched highest-precedence first, so the winner takes the device and
the rest are skipped.
[*terminal.deferred-starts-are-dispatched-highest-precedence-first]
Handovers, below, pick one winner outright.

Within the Phase 2 boot plan the order is the dependency graph's, not
precedence's [*terminal.the-boot-plan-orders-by-the-graph-not-precedence]
— a boot-plan terminal service is being written over by
peinit's own progress output anyway, which is what `boot:settled`
exists to avoid.

<!-- lint-disable-next-line line-length -->
## Handing it on [*terminal.a-tty-released-waiter-is-started-when-the-terminal-comes-free]

A service that names a terminal and carries the `tty:released` trigger
(§3.4) is started when that terminal comes free. peinit watches every
transition out of a holding state, so it does not matter whether the
holder exited cleanly, crashed, was stopped by an administrator or was
evicted by a conflict — all four free the device
[*terminal.every-exit-from-a-holding-state-frees-the-device], and the
waiter is
wanted after a crash more than after a clean exit, not less.

Which terminal was released is recorded **on the transition**, not looked
up afterward, and that is load-bearing rather than tidy.
[*terminal.the-released-terminal-is-recorded-on-the-transition] A service
whose
definition has been removed loses its table entry the moment it reaches a
state that does not retain one (§3.8) — the same moment it releases its
terminal. Asking afterward which `TTYPath` it had gets nothing.

That is not a corner case. It is precisely what a service that retires
itself does: remove its own definition so it never runs again, then exit.
First-boot setup is exactly that, and before the release rode on the
transition it ended by telling the operator they could sign in, on a
console it had vacated and nobody had been given.

One winner per release, chosen by precedence.
[*terminal.one-winner-per-release] Starting the whole queue
would have every loser immediately skipped again by the rule above:
the same outcome, reached noisily, with a skipped-looking service for
each.

Two services are never offered a terminal:

- **The one whose exit freed it.**
  [*terminal.the-service-whose-exit-freed-the-terminal-is-not-offered-it]
  It has the strongest claim on its own
  console, so without this exclusion the highest-precedence holder
  restarts itself on every exit — a restart policy with no budget, and
  one no waiter behind it ever gets past. Relaunching a service that
  stopped is `RestartPolicy`'s job (§6.4).
- **One whose definition has been removed** (§3.8), since it cannot be
  started at all.
  [*terminal.a-service-with-a-removed-definition-is-not-offered-it]

A waiter is usually sitting in Skipped when its turn comes, from the
moment it lost the terminal. The handover starts it as an explicit start
would, which is the transition that takes a service out of Skipped
(§6.2) and re-evaluates its conditions.
[*terminal.the-handover-starts-the-waiter-as-an-explicit-start-would]

## Conflicts

`Conflicts` (§7.1) fails *both* services when they meet in one boot. The
services that want a terminal are the interactive ones — a sign-in prompt,
an installer, a first-boot setup flow — so that answer is a machine with
no way into it.

A dependency says the wrong thing twice. A sign-in prompt does not need
first-boot setup to have *run*; it needs the console to be free, and a
setup flow that failed frees it as well as one that succeeded.
And naming the holder would mean rewriting every waiter each time the
set of things that might take the console changed.

What is left is a queue on a device, which is what this is.

## Fixed system-console endpoints

When the LCS boundary reads service definitions, `/dev/console` in `TTYPath`
is
resolved from the last entry in `/sys/class/tty/console/active`. A `tty0`
endpoint
is pinned to `/dev/tty1`; an explicit `/dev/tty0` is also pinned to
`/dev/tty1`.
The selected system-console endpoint is cached for the boot so registry
reloads
and foreground VT changes do not move existing sessions. Resolution failure
makes the affected definition undecodable, rather than silently opening a
moving
alias. Arbitration and process launch use the same resolved path.

On terminal main-process exit the runtime kills the exiting job's cgroup
before
releasing the terminal or scheduling its successor. Descendants belong to the
session that ended, not the next sign-in attempt. Other terminal services and
ordinary
daemon lifecycle semantics are unaffected. An error killing the cgroup fails
terminal-event processing; a new prompt must not be scheduled through that
error.
