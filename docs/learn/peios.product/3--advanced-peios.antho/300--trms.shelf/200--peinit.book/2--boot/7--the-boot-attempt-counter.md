---
title: The Boot Attempt Counter
description: The on-disk counter that turns a repeated failure into an escalation — its cycle, and the threshold that reaches Recovery.
---

The counter is what turns a repeated failure into an escalation. peinit
keeps it at `/.peinit/boot-attempts`, as a plain decimal integer in a
file on the root filesystem [*attempts.the-counter-is-a-file-on-the-root]
— deliberately not in the registry, because the registry can be the
reason the boot is failing.

## The cycle

peinit reads the counter at startup, before selecting a boot mode. The
recovery threshold is evaluated against this **pre-increment** value, so
a default threshold of 3 admits exactly three boot attempts before
recovery. [*attempts.the-threshold-is-checked-pre-increment]

- **Absent file:** treated as 0. [*attempts.an-absent-counter-is-zero]
- **Unreadable, empty, non-decimal, carrying trailing non-whitespace
  data, or overflowing the counter representation:** recovery. A counter
  that cannot be read cannot be trusted to escalate.
  [*attempts.an-unreadable-counter-is-recovery]

peinit increments once per boot, after the root is known writable and
before Phase 2 begins. [*attempts.the-counter-increments-once-per-boot]
Incrementing before the root is known writable would silently lose the
increment on a read-only root and defeat escalation entirely.

The increment happens after the mount, seed, machine ID and clock steps
rather than immediately after the writability probe, because reading the
kernel command line requires `/proc`. One consequence is that a recovery
entered from a mount failure, a machine ID failure, or an unreadable
command line does not advance the counter.
[*attempts.an-early-recovery-does-not-advance-the-counter]

A write failure — a full disk, say — is treated as a counter of 0 and
the boot continues. A failure to record an attempt is not itself a
reason to escalate. [*attempts.a-write-failure-reads-as-zero]

The counter resets to 0 on a successful Full or Safe boot, after the
grace period. [*attempts.a-successful-boot-resets-the-counter]

The control socket's `boot` command reports the count this boot began
with, the threshold, and whether this boot has reset the counter yet
(§10.8).

## Recovery threshold

The threshold is `peios.bootattempts=N` on the kernel command line,
defaulting to 3. [*attempts.the-default-threshold-is-three] It is a
command-line value rather than a registry one because the check runs in
Phase 1, before registryd is serving.

`peios.bootattempts=0` disables the check entirely — the escape hatch
for a system whose counter is itself the fault.
[*attempts.a-threshold-of-zero-disables-the-check]

`peios.recovery=1` forces recovery without consulting the counter, but
does not suppress the increment.
[*attempts.forced-recovery-still-increments]

> [!NOTE]
> The counter catches boots where peinit runs but the system never
> reaches health — a crash-looping Critical service being the usual
> case. It deliberately does not try to catch a peinit too broken to
> reach its own increment. Recovery mode is itself a peinit mode, so a
> peinit that cannot start cannot deliver recovery either, and a higher
> count could not help. That failure belongs to binary integrity, not to
> the counter.
