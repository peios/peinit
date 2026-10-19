---
title: The Boot Query
description: What `boot` reports about the current boot — its mode and why, the boot attempt count, and whether the boot has counted as a success — and where peinit gets each answer.
---

`boot` tells a client how the machine booted, so that a settings app or
an administrator at a shell can see that it is in Safe mode, why, and
how many more failed boots stand between it and recovery. The wire shape
is PSPU §4.15. This page is where peinit gets each answer.
[*control.boot.reports-the-current-boot]

It takes no arguments and changes nothing. It is checked against the
control descriptor for `SYSTEM_QUERY_STATUS`, which the default grants
every authenticated principal (§4.7), and it is answered during
shutdown (§10.2). [*control.boot.is-checked-for-query-status]

## Mode and reason

`mode` is the mode Phase 2 booted in, after any downgrade: `full` or
`safe` (§2.6). [*control.boot.mode-is-the-mode-phase-2-booted-in] A
Recovery boot never reaches the runtime — peinit runs the recovery shell
instead (§2.8) — so there is no control socket to ask, and `boot` never
answers `recovery`. [*control.boot.recovery-is-never-answered]

`reason` says why.
[*control.boot.reason-distinguishes-normal-requested-and-downgraded]

| `reason` | When |
|---|---|
| `normal` | A Full boot. |
| `requested` | A Safe boot asked for with `peios.safemode=1`. |
| `safe_mode_downgrade` | A Full boot that Phase 2 downgraded to Safe: a Critical service in a dependency cycle, or in an unresolvable conflict, at boot (§2.6). |

For a downgrade, `downgrade` carries every finding that forced it, in
the words the console line and the `peinit.boot.downgraded` event use
— all of them, not the first. [*control.boot.downgrade-lists-every-finding]
peinit keeps them from the boot plan, the one place they survive the
Safe-mode rebuild.

## Attempts

`attempts` is the boot attempt count (§2.7) as Phase 1 checked it
against the recovery threshold: the value in `/.peinit/boot-attempts`
before this boot's increment, or 0 if the increment could not be
written, which is how Phase 1 treats a counter it cannot advance.
[*control.boot.attempts-is-the-count-phase-1-checked] It is the count
at the start of this boot, so it does not drop to 0 when this boot is
confirmed; `confirmed` says that.

`max_attempts` is the threshold: `peios.bootattempts=N`, or 3 when the
command line does not set it. 0 means the check is off.
[*control.boot.max-attempts-is-the-threshold]

## Confirmation

A boot is **confirmed** when it has counted as a success: every Critical
service has held a dependent-satisfying state for `BootSuccessGrace`
seconds without a break, and peinit has reset the counter to 0 (§2.5).
`grace_seconds` is that grace. The other fields say where the boot
stands. [*control.boot.confirmation-states]

| `confirmed` | `waiting_on` | `confirms_at` | `confirm_error` | Meaning |
|---|---|---|---|---|
| false | the Critical services not holding | null | null | The boot is waiting for them. |
| false | empty | a time | null | Every Critical service holds; if they keep holding, the boot counts then. |
| true | empty | null | null | The boot has counted, and the counter is 0. |
| false | empty | null | why | The grace ended but the counter could not be reset. The next boot will count this one as a failure. |

`confirmed` is true only once the reset has been written.
[*control.boot.confirmed-means-the-counter-was-reset] A grace that ends
with the write failing — a full disk, say — is not a confirmed boot: the
file still holds this boot's increment, and the next boot will count it,
whatever its services did. The failed reset is not otherwise reported,
so this is where an administrator finds it.

A confirmed boot stays confirmed. A Critical service that fails later is
handled by the ordinary Critical failure path — restart budget, then
reboot (§2.6) — not by un-counting the boot.
[*control.boot.a-confirmed-boot-stays-confirmed]

A boot with no Critical services holds from the moment Phase 2 runs, so
`confirms_at` is known at once.
