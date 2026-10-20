---
title: Power Loss and Corruption
description: The three things peinit writes and how each is made to survive a power loss — plus damaged files, timers, and an interrupted shutdown.
---

## What peinit writes

Three things, and all three are written to survive an unexpected loss of
power: [*powerloss.the-three-things-peinit-writes]

| What | Where | How |
|---|---|---|
| Boot attempt counter | `/.peinit/boot-attempts` | A plain integer, rewritten each boot. |
| Local machine ID | `/lcl/etc/machine-id` | Temporary file, flush, rename. |
| Random seed | `/var/state/peinit/random-seed` | Temporary file on the same filesystem, flush, atomic rename, directory flush. |

The seed and the machine ID are atomic in the sense that matters: a
reader sees either the old value or the new one, never a partial write.
[*powerloss.the-seed-and-the-machine-id-are-never-seen-half-written]

## Reading a damaged file

The three behave differently on finding something they cannot use, and
the differences track how bad each situation is.

**The counter** is the strictest. Absent means zero. But unreadable,
empty, non-decimal, carrying trailing data, or overflowing all send
peinit to recovery mode. A counter that cannot be trusted cannot
escalate, and the whole point of it is escalation — so failing to read
it is treated as though it had already escalated.

**The machine ID** is regenerated. Absent, empty, all zeroes, the wrong
length, not hexadecimal, or missing its trailing newline all produce a
fresh identifier, recorded as a warning. It is an opaque install
identifier, not a security principal, and a new one is a smaller problem
than no boot. An unreadable file or an unwritable path is fail-soft too
— the boot continues on an ephemeral identifier, recorded as a warning.
Only a failure to *generate* one sends peinit to recovery, because at
that point there is no identifier to continue with (§2.3).

**The seed** is entirely fail-soft. Absent, empty, oversized, or
unrestorable all continue the boot. An empty or oversized seed is
recorded as an error on the way past (§2.3); none of them stops the
boot. A system with no entropy
cache still boots; it starts with less entropy, which is the image
builder's problem to solve with a hardware or virtio RNG.

## The registry

peinit does not write service state to the registry, apart from timer
last-run timestamps.
[*powerloss.no-service-state-is-written-to-the-registry] Everything
else about a service's runtime state lives in memory and is rebuilt from
the definitions on the next boot, which means a power loss cannot leave
peinit's own state inconsistent — there is no state on disk to be
inconsistent with.

Registry consistency across power loss is loregd's concern, and its
recovery paths are what the recovery shell offers (§2.8).

## Timers across a loss

A persistent timer's last-run timestamp is written after the firing and
after the start is initiated, not after the service completes (§9.3).
[*powerloss.the-last-run-timestamp-is-written-when-the-start-is-initiated]
A power loss mid-run therefore does not re-trigger on the next boot — the
run was attempted, not missed.
[*powerloss.a-power-loss-mid-run-does-not-re-trigger]

A power loss between the firing and the write does re-trigger, which is
the right way round: one extra run is better than a silently skipped
one. [*powerloss.a-power-loss-before-the-write-does-re-trigger]

## Shutdown that never finishes

If power is lost during the graceful sequence, the effect depends on how
far it got. Before step 7, the filesystems are still mounted read-write
and the next boot behaves like any unclean shutdown. After step 7, the
root has been remounted read-only and everything else unmounted, so
there is nothing outstanding to lose.
[*powerloss.after-step-7-the-root-is-read-only-and-nothing-is-outstanding]

The counter was incremented at the start of the boot that is now ending,
and a successful boot resets it *during* that boot, once the success
grace period has been held. So a power loss during the shutdown of a
boot that succeeded leaves the counter at zero; one during the shutdown
of a boot that never reached success leaves it advanced.
[*powerloss.only-a-boot-that-never-succeeded-leaves-the-counter-advanced]
Enough of those in a row reach the recovery threshold, which is the
intended behavior: a machine that keeps failing to reach a successful
boot is a machine an administrator should be looking at.
