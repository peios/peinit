---
title: Recovery Mode
description: The last resort — no TCB guarantee, an administrator shell, registry recovery limits, and console access.
---

Recovery mode is the last resort. There is no TCB guarantee and no
degraded boot to speak of — it is a maintenance environment that hands
the administrator an unrestricted SYSTEM shell on the console.

## Entry

- The boot attempt counter reaching the threshold (§2.7).
- `peios.recovery=1` on the kernel command line.
- Any of the Phase 1 failures listed in §2.3, most importantly a
  registryd that will not start or will not serve.
- A Phase 2 registry read that fails or times out, or invalid boot
  configuration.
- A required provisioned path that cannot be created or secured.
- The runtime loop failing on an error about supervision itself: the
  event wait, event source registration, a listener, the event ring
  refusing even the small `peinit.event.dropped`, shutdown finalization. An
  error peinit can attribute to one service is contained to that
  service instead and does not enter recovery (§8.2).

## What peinit does

peinit records the reason as a `peinit.recovery.entered` KMES event —
`essential`, so no emission policy switches it off — with the reason in
`outcome.reason` and what went wrong, in the error's own words, in
`outcome.detail`; a Phase 2 plan that would not build adds its finding
as a `peinit.graph.validation.failed` under phase `phase2-boot`
[*recovery.the-reason-is-audited], then:

1. Completes Phase 1 steps 1–5 if they have not been reached yet.
2. Ensures the base registry structure exists, so the shell sees a
   normal layout even on a system that has never been provisioned.
   [*recovery.the-base-registry-structure-is-ensured]
3. Attempts to start registryd. A failure here is ignored — recovery
   delivers a shell whatever registryd's state.
   [*recovery.a-registryd-failure-does-not-prevent-the-shell]
4. Skips all Phase 2 services. [*recovery.no-phase-2-service-starts]
5. Starts a shell on `/dev/console` from a compiled-in definition, with
   no registry dependency: `/bin/recsh` if it is present and
   executable, otherwise `/bin/sh` [*recovery.recsh-is-preferred-over-sh],
   running as SYSTEM with a fixed environment of `PATH=/sbin:/bin`,
   `TERM=linux` and `HOME=/`. [*recovery.the-shells-environment] peinit
   does not care where either binary comes from.
6. Logs the failure reason to the console.
   [*recovery.the-reason-reaches-the-console]

If the shell exits, peinit respawns it. Recovery never exits to an
unmanaged PID 1. [*recovery.the-shell-is-respawned]

If neither `/bin/recsh` nor `/bin/sh` can be exec'd, peinit cannot
deliver a shell at all. It logs the reason to the console, syncs, and
halts — PID 1 exiting would panic the kernel.
[*recovery.no-shell-at-all-syncs-and-halts] A missing shell is a
binary-integrity failure and sits outside the boot-attempt machinery's
remit.

The shell receives `/dev/console` duplicated onto its standard streams,
but peinit does not call `setsid()` or acquire a controlling terminal
for it. The shell is not a session leader, so job control is not
available in the recovery shell.
[*recovery.the-shell-is-not-a-session-leader]

Step 1 runs on every entry. The steps are individually idempotent, so
completing them and ignoring the failures is what "if they have not been
reached yet" amounts to in practice — including the step that failed,
which by the time the operator has a shell can well succeed.
[*recovery.phase-1-steps-are-retried-idempotently]

Step 3 runs only if Phase 1 has not already reached its own registryd
start, and it runs at most once.
[*recovery.recovery-starts-at-most-one-registryd] A recovery entered from a
Phase 2,
provisioning or runtime failure starts no registryd: Phase 1 already
did, and its activation is retained for the runtime. A recovery entered
from the registryd start *failing* also starts none — the attempt has
been made, and a process can have forked before the failure was
reported. In both cases a second daemon would bind over the first's
notify socket, which succeeds rather than reporting `EADDRINUSE`
because the bind unlinks the path first, and would then open the same
hive files behind the first daemon's back.

Where recovery does start one, it uses the settings this boot parsed, so
`peios.notifysocket=` and `peios.quiet` apply to it.
[*recovery.a-recovery-registryd-uses-this-boots-settings]

> [!NOTE]
> Recovery mode is not a degraded boot; it is a maintenance environment.
> There are no security protections beyond what the kernel provides. The
> administrator has a SYSTEM shell and the corresponding responsibility.

## Registry diagnosis and recovery limits

Recovery provides a console maintenance shell when one is available as
described above; it does not define an offline storage-repair interface
for the registryd provider. Preserve the console failure reason
and source startup output, then identify the deployed provider and version,
its hive declarations, and the actual database paths. See
[LCS and sources](~peios/registry-sources/overview).

Entering Recovery does not establish that the source is stopped. As
explained above, a registryd started or attempted earlier in the boot can
remain active. The loregd [exit
contract](~peios/loregd/startup/exit-and-shutdown)
also permits a source to keep running after storage-operation failures.
Do not launch a second source against the same hive files.

The [source-checked loregd
implementation](~peios/loregd/startup/command-line#unsupported-recovery-switches)
takes one or more `HiveName=DatabasePath` declarations and rejects arguments
without `=`. It has no offline inspection, backup-recovery, or
database-clearing
switches and [does not create an automatic startup
backup](~peios/loregd/startup/startup-sequence#backup-and-repair-limits).
Startup WAL recovery and orphan cleanup are not backup restoration or a
database-corruption repair facility. The linked references pin these findings
to the implementation revision checked; identify the installed provider and
revision before choosing a recovery procedure.

Offline inspection, repair, or database replacement therefore needs a
procedure supported by the installed provider and version. Establish the
source's state and verify the existence and coverage of a recovery copy
before planning a replacement. This manual supplies no offline stop,
repair or replacement sequence. Role definitions can re-supply service
configuration; they do not constitute a backup of every registry value.

The ordinary [Backup and
restore](~kernel/internals/lcs/backup-and-restore/overview)
workflow is separate: LCS coordinates operations through a working source.
[Backup](~kernel/internals/lcs/backup-and-restore/backup) requires a read-only snapshot
transaction; [restore](~kernel/internals/lcs/backup-and-restore/restore) replaces a
target
subtree, including descriptors, in a read-write transaction. Neither is a
way to repair a source that cannot serve those operations.

> [!NOTE]
> `/bin/recsh` exists so a system can ship a purpose-built recovery
> shell — one that bundles the offline registry tools, presents
> guidance, or curates a command set — without making it mandatory.
> `/bin/sh` is the floor where it is absent. peinit treats both as
> opaque executables.

## Remote recovery

Recovery requires console access: physical, IPMI, or serial. Two
post-v1 features address headless servers — rolling the registry back to
a last-known-good state from the recovery shell, and an emergency sshd
started without registry involvement.
