---
title: Boot Modes
description: Full, Safe,  and Recovery — the escalation path from normal operation to last-resort maintenance, and what causes each downgrade.
---

peinit boots in one of three modes, forming an escalation path from
normal operation to last-resort maintenance.

```text
Full boot ---+-- success ----------------> counter reset, operational
             |
             +-- cycle w/ Critical ------> Safe mode (no reboot)
             |
             +-- conflict w/ Critical ---> Safe mode (no reboot)
             |
             +-- Critical failure -------> sync + reboot --+
                                                           |
Safe boot ---+-- success ----------------> counter reset,  |
             |   operational (reduced)                     |
             +-- Critical failure -------> sync + reboot --+
                                                           |
                             counter increments <----------+
                                        |
                             counter >= N ---------> Recovery mode
                             Phase 1 failure ------> Recovery mode
```

## Full mode [*mode.full-is-the-default]

The default. Every boot-triggered service starts in dependency order, as
§2.5 describes.

## Safe mode

Safe mode starts a reduced set. Eligibility is a filter *within* the
boot-triggered set, not a replacement for it: a service with no `boot`
trigger does not auto-start in Safe mode whatever its `SafeMode` or
`ErrorControl` says, and remains demand-only.
[*mode.safe-does-not-start-a-service-with-no-boot-trigger] Within the
boot-triggered set, two categories are eligible:

- **Critical services** (`ErrorControl=Critical`) start. If one fails,
  the ordinary Critical failure path applies — restart budget, reboot,
  counter increment, eventually recovery.
  [*mode.safe-starts-critical-services]
- **`SafeMode=1` services** are attempted best-effort. If one fails,
  Safe mode continues without it.
  [*mode.safe-attempts-safemode-services-best-effort]

`ErrorControl=Critical` implies `SafeMode`, so a Critical service does
not have to declare both. [*mode.critical-implies-safemode]

### What caused the downgrade

The rebuild discards the Full-mode graph, so a service that forced Safe
mode and is then *excluded* from the rebuilt graph is never entered into
the blocked set and never marked Failed. That is deliberate: Safe mode
was never going to start it, and a Failed state would say something
about its own health that is not true. `status` should keep meaning
"this service is broken".
[*mode.a-service-excluded-from-the-rebuild-is-not-marked-failed]

A service that survives into the rebuilt graph gets no such protection.
Two `Critical` services that require each other are both eligible in
Safe mode, so the rebuild contains the same cycle and blocks them both,
exactly as it would anywhere else — and there is no order that could
have started either.
[*mode.a-service-that-survives-the-rebuild-can-still-fail-there]

The reason is therefore recorded at **boot level** rather than per
service. Every finding that forced the downgrade — each critical cycle,
each critical boot conflict — is written to the console and emitted as a
`peinit.boot.downgraded` KMES event naming the services involved, an
`essential` type the emission policy cannot switch off.
[*mode.the-downgrade-reason-is-recorded-at-boot-level]

All of them are reported, not only the first. A machine can be downgraded
by a cycle *and* a conflict at once, and an operator who fixed only the
one they were shown would reboot straight back into Safe mode.
[*mode.every-downgrade-finding-is-reported]

They stay available for the rest of the boot: the control socket's
`boot` command reports the mode, why, and every finding (§10.8).

peinit rebuilds the dependency graph from scratch using only the
eligible services. Dependencies on excluded services are dropped: if A
depends on non-Critical B and B is excluded, A's dependency on B does
not exist in the Safe mode graph.
[*mode.safe-drops-dependencies-on-excluded-services] This is what makes
Safe mode useful — it is a graph in which the broken parts of the
configuration are not present, rather than a graph in which they
are present and failing.

A successful Safe boot resets the boot attempt counter.
[*mode.a-successful-safe-boot-resets-the-counter]

> [!NOTE]
> Safe mode is purely a boot-sequencing concern. Once booted, an
> administrator starts anything by hand exactly as in Full mode. It is
> for a system whose configuration is broken but whose TCB is healthy.

### Entry

- **A cycle involving a Critical service at boot.** Graph validation
  detects it and peinit downgrades in place, without rebooting — the
  cycle is a configuration error, and rebooting would find it again.
  [*mode.a-critical-cycle-downgrades-in-place]
- **An unresolvable conflict involving a Critical service at boot.**
  Same reasoning. [*mode.a-critical-conflict-downgrades-in-place]
- **`peios.safemode=1`** on the kernel command line.
  [*mode.safemode-can-be-forced-from-the-command-line]

Safe mode is not entered because a Critical service crashed at runtime.
That follows the ordinary path: restart budget, reboot, counter
increment, recovery.
[*mode.a-runtime-critical-failure-does-not-enter-safe-mode]

## Console output

`peios.quiet=N` bounds what peinit writes to the console:

| Value | Behavior |
|---|---|
| `0` | Write unconditionally. [*quiet.zero-writes-unconditionally] |
| `1` | Do not write to a terminal held as the controlling terminal of a running service, except to announce loss of the system. This is the default. [*quiet.one-is-the-default-and-respects-terminal-ownership] |
| `2` | Additionally drop ordinary progress everywhere, while still emitting errors. [*quiet.two-drops-progress-but-not-errors] |

The two rules are independent, and an error is never less visible at `2`
than at `1`. A terminal is matched by device rather than by path, since
`/dev/console` and `/dev/ttyS<n>` can name the same device
[*quiet.a-terminal-is-matched-by-device-not-path]; where the device
cannot be determined, peinit treats the terminal as free and writes to
it. [*quiet.an-undeterminable-device-is-treated-as-free] Guessing wrong
in that direction costs a scrambled line, which is recoverable;
guessing wrong the other way costs the operator their console output at
the moment the machine is least able to explain itself.
Suppressed messages are discarded rather than buffered.
[*quiet.suppressed-messages-are-discarded]

The autorun step (§2.3) bypasses the policy: a script that ran that
early and went wrong is worth interrupting a sign-in prompt for.

## Kernel command line

| Parameter | Effect |
|---|---|
| `peios.safemode=1` | Force Safe mode. |
| `peios.recovery=1` | Force recovery mode regardless of the counter. [*cmdline.recovery-forces-recovery-mode] |
| `peios.bootattempts=N` | Set the recovery threshold; `0` disables the check. [*cmdline.bootattempts-sets-the-threshold-and-zero-disables-it] |
| `peios.quiet=N` | Console verbosity, as above. |
| `peios.notifysocket=PATH` | Override the notification socket path. [*cmdline.notifysocket-overrides-the-socket-path] |

A malformed value is ignored in favor of the default rather than
failing the boot. [*cmdline.a-malformed-value-falls-back-to-the-default]
Nothing exists this early to report a diagnostic to.
