---
title: Phase 2
description: The registry-driven phase — reading the definitions, building and validating the graph, starting it, and deciding the boot succeeded.
---

With registryd serving, peinit reads the service graph and boots the
system from it. Phase 2 is entirely registry-driven.

## Reading the definitions

peinit reads every key under `Machine\System\Services\`.
[*phase2.definitions-come-from-the-services-key] The reads are bounded by
LCS's request timeout: if registryd hangs mid-read, peinit receives
`ETIMEDOUT` and enters recovery. [*phase2.a-registry-read-timeout-is-recovery]

Decoding is per key. A definition that fails to decode — an invalid
service name, a malformed trigger, an unclosed quote in a command, a
`registry:` check naming an uncacheable key, a duplicate known field, an
unrecognised value for an enumerated dword — fails that service, which
is marked Failed with cause `ValidationError`.
[*phase2.an-undecodable-definition-fails-only-that-service] The boot
proceeds with every other definition, and anything that depended on the
failed service fails in turn through the ordinary dependency propagation.

peinit holds no definition for such a service, so the entry behind it
is a placeholder: `status` reports it Failed with cause
`ValidationError`, marked definition-removed (§3.8), with the decode
error as its description. `start`, `restart`, `reload` and `reset` are
refused with `UNKNOWN_SERVICE`, as for any definition-removed service.
The placeholder leaves with its key: a `reload-config` that re-reads a
repaired key restores the entry, and one that finds the key deleted
discards it, since Failed does not retain a definition-removed entry.

Only services carrying a `boot` trigger are root candidates.
[*phase2.only-boot-triggered-services-are-roots] A service with no
triggers is demand-only and is not a root, though it can still be pulled
into the boot transaction as somebody's dependency.
[*phase2.a-demand-only-service-can-still-be-pulled-in] A service with
`Disabled=1` is excluded from the boot graph entirely, but its definition
is still loaded into the in-memory model so it can be started by hand
later. [*phase2.a-disabled-service-is-loaded-but-not-booted]

## Building and validating the graph

The boot graph is every boot-triggered root candidate plus the
transitive closure of their `Requires`, `BindsTo`, and existing
non-disabled `Wants` dependencies.
[*phase2.the-boot-graph-is-the-roots-transitive-closure] `Requires` and
`BindsTo` pull their target in even if the target has no `boot` trigger.
[*phase2.a-hard-dependency-pulls-in-an-untriggered-target] `Wants`
targets come in as best-effort members; missing or disabled ones are
ignored. [*phase2.a-missing-wants-target-is-ignored] A missing or
disabled `Requires` or `BindsTo` target blocks the dependent with cause
`DependencyFailure`, and that blocking propagates.
[*phase2.a-missing-hard-dependency-blocks-and-propagates]

peinit topologically sorts the graph and validates it before starting
anything. The rules are in §7.2; the outcomes that matter here are:

- **A cycle** fails every service in it. A cycle involving a Critical
  service downgrades the boot to Safe mode without rebooting.
  [*phase2.a-cycle-fails-every-service-in-it]
- **An unresolvable conflict** fails both services. Same downgrade if
  either is Critical. [*phase2.a-conflict-fails-both-services]
- **A missing `Requires` target** fails the dependent.
- **Warnings** — a `Readiness=Alive` service with dependents that
  require it — are logged and do not prevent boot: a `[ WARN ]`
  `peinit warning:` line on the console and a
  `peinit.graph.validation.warned` event under phase `boot`, whether or
  not the same graph also has
  findings. [*phase2.a-validation-warning-does-not-prevent-boot]

## Starting

peinit walks the graph and starts services, parallelising wherever the
graph allows, up to a configurable limit:

| Key | Default | Meaning |
|---|---|---|
| `Machine\System\Boot\MaxParallelStarts` | 10 | Services starting concurrently. [*phase2.maxparallelstarts-bounds-concurrency] |

An absent key uses the default. A value of zero, a type mismatch, or a
malformed payload is invalid boot configuration and sends peinit to
recovery [*phase2.an-invalid-maxparallelstarts-is-recovery] — running the
scheduler with an effective limit of zero would hang the boot rather than
fail it.

As each service reaches a dependent-satisfying state — Active for
Simple, Completed for Oneshot with or without `RemainAfterExit`, Skipped
for a service whose conditions did not hold — its dependents become
eligible and join the start queue.
[*phase2.a-satisfying-state-releases-dependents] A Oneshot without
`RemainAfterExit` passes through Completed, releasing its dependents, and
then goes Inactive.
[*phase2.a-oneshot-releases-dependents-before-going-inactive]

Dependents blocked on a `Requires` or `BindsTo` target wait for that
target to reach a satisfying state.
[*phase2.a-hard-dependent-waits-for-a-satisfying-state] Dependents
blocked on a `Wants` target wait only for it to reach *any* terminal
state, satisfying or not — which is what makes `Wants` ordering rather
than dependency. [*phase2.a-wants-dependent-waits-only-for-a-terminal-state]

### The typical order

Nothing below is hardcoded. It falls out of the dependency graph that
the standard role definitions produce, and an administrator who changes
a dependency gets a different order.

1. **eudev** — device management. SYSTEM, with `RequiredPrivileges`
   stripped to the minimum it needs. Starts before authd exists.
2. **lpsd** — the local identity database. Depends only on registryd.
3. **authd** — the identity authority. Depends on registryd and lpsd.
   Once it is ready, token minting is available.
4. **eventd** — logging and audit. Services started before it sign in to
   peinit's pre-eventd buffer.
5. **Networking** — the first service to receive a token from authd.
6. **Application services**, in dependency order.
7. **Sign-in services**, last, so the system is operational before it
   accepts a session.

### The bootstrap matrix

| Service | Phase | Identity | Token from | Readiness | ErrorControl |
|---|---|---|---|---|---|
| registryd | 1 | SYSTEM | minted by peinit | Notify | Critical |
| eudev | 2 | SYSTEM | minted by peinit, privileges stripped | Alive | Normal |
| lpsd | 2 | SYSTEM | minted by peinit | Notify | Critical |
| authd | 2 | SYSTEM | minted by peinit | Notify | Critical |
| eventd | 2 | SYSTEM | minted by peinit | Notify | Critical |
| networking | 2 | service token | authd | Notify | Normal |
| sshd | 2 | service token | authd | Alive | Normal |
| application services | 2 | service token | authd | per-service | Normal |

## Deferred starts

A service whose trigger is `boot:settled` is not part of the boot plan
at all. It is not a root, it does not consume the parallel-start budget,
it is not counted toward boot success, and it cannot block or delay
anything. peinit starts it after the plan, once the boot has stopped
moving. [*settle.a-deferred-service-is-outside-the-boot-plan]

"Stopped moving" is precise: every service in the plan — those that were
started and those that were blocked — is in a state it will not leave
without help. Active, Completed, Failed, Skipped, Abandoned, and Inactive
all count as settled. [*settle.which-states-count-as-settled] Starting,
Reloading, Stopping and **Backoff** do not, because a service between
restart attempts is going to produce more output.
[*settle.backoff-is-not-settled]

A deadline bounds the wait:

| Key | Default | Meaning |
|---|---|---|
| `Machine\System\Boot\SettleTimeout` | 5 | Seconds before deferred services start regardless. [*settle.settletimeout-bounds-the-wait] |

The deadline is measured from the moment the plan was observed. An
absent key uses the default; a type mismatch or a bad length sends
peinit to recovery. [*settle.an-invalid-settletimeout-is-recovery] Zero
is legal and means "start on the next turn, settled or not".
[*settle.a-settletimeout-of-zero-is-legal]

Whichever comes first — the set settling or the deadline expiring — the
deferred services start, once, each independently.
[*settle.deferred-services-start-once-each] A start that fails is
recorded and dropped: one refusing service does not stop the others and
does not affect the boot.
[*settle.a-failed-deferred-start-does-not-affect-the-boot] The dispatch
carries a flag saying whether the deadline expired rather than the set
settling, so a service that cares whether the boot was still moving can
be told. [*settle.the-dispatch-says-whether-the-deadline-expired]

> [!NOTE]
> The motivating case is a console sign-in prompt being scribbled over by
> peinit's own progress messages. That is a scheduling preference, not a
> dependency — the prompt does not *require* anything to have started,
> it wants the console to itself. Expressing it as a dependency
> would mean naming every service that might print, which changes every
> time the system does.

## Boot success

A boot is successful once every Critical service has held a
dependent-satisfying state continuously for a grace period:

| Key | Default | Meaning |
|---|---|---|
| `Machine\System\Boot\BootSuccessGrace` | 30 | Seconds of held health before the boot counts. [*success.the-grace-period-must-be-held-continuously] |

The criterion is *satisfying*, not Active. A Critical Oneshot reaches
Completed and never reaches Active, so a test for Active would make such
a service unable to ever mark a boot successful. Skipped counts too.
[*success.the-criterion-is-satisfying-not-active]

Success resets the boot attempt counter to zero (§2.7). The control
socket's `boot` command says where a boot stands: which Critical
services it is waiting for, or when it will count, or that it has
(§10.8).

## Failure summary

| Failure | Response |
|---|---|
| A service definition fails to decode | That service Failed with cause `ValidationError`; the rest of the boot proceeds |
| A registry read times out | Recovery |
| Invalid `MaxParallelStarts` or `SettleTimeout` | Recovery |
| A dependency cycle | All services in it Failed; Safe mode if any is Critical (see below) |
| An unresolvable conflict | Both Failed; Safe mode if either is Critical (see below) |
| A missing `Requires` target | The dependent Failed |
| A Critical service fails during boot | Restart budget, then reboot |
| A non-Critical service fails | Failed; its `Requires` dependents fail; the rest continue |
| authd unavailable when a service needs a token | That service Failed |

The two Safe-mode rows carry a caveat. When the downgrade fires, peinit
rebuilds the graph in Safe mode and discards the Full-mode one, so the
services that caused it are **not** marked Failed — they are never
entered into the blocked set at all. What records them is the boot-level
downgrade finding described in [Boot modes](~peios/boot/boot-modes).
