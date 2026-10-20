---
title: Configuration Generations
description: peinit operates on snapshots rather than a live registry view — the boot and activation generations, field mutability, and pinning.
---

peinit operates on snapshots, not on a live view of the registry. Two
generation concepts govern when a registry change takes effect.

## The in-memory model

peinit maintains a full in-memory model of every service definition. The
registry is read synchronously exactly twice: during Phase 2 boot,
before meaningful supervision has started, and during a reload-config,
which an administrator initiated and which is bounded. At all other
times peinit works from the model.
[*confgen.peinit-answers-from-the-model-rather-than-the-registry]

> [!NOTE]
> The model exists because the event loop cannot block on a userspace
> service during normal supervision. In single-threaded PID 1
> a blocking syscall blocks everything — child reaping, watchdog expiry,
> shutdown signals, every other event stops while the syscall is stuck.
> Reading the registry means waiting on registryd, and registryd is a
> service peinit supervises.

Change notifications arrive as events on a pollable descriptor. peinit
subscribes to `Machine\System\Services\` and `Machine\System\Init\` at
boot, [*confgen.the-watch-covers-services-and-init] using the LCS watch
mechanism — a persistent subscription that
delivers change events on a key descriptor, with an OVERFLOW event when
the kernel-side queue is exceeded (Peios Kernel TRM §5).

Any drained watch event triggers a full reload of the configuration,
not a targeted re-read of the changed key. That covers the OVERFLOW case
by construction, and it is why an administrator writing one value causes
every definition to be re-read.

## The boot generation

At the start of Phase 2, peinit reads all definitions, builds the
dependency graph and validates it. The plan and the graph are fixed at
that point, and the boot executes against them.

The watches are armed as the event loop starts, which is after the plan
is fixed but while boot-plan services are still starting. A registry
write during that window — from an install script, a post-hook, a
package transaction — triggers a reload like any other, and the reload
applies like any other, with one exception: a boot-plan service whose
launch has not yet been attempted keeps the definition the plan was
built from, and the new one is recorded as pending, exactly as for a
running service. The boot executes against its snapshot (§2.5). Such
services are listed under `deferred` in the reload's summary, said on
the console, and recorded as `peinit.config.reload.deferred`. Once every
planned launch has been attempted or decided against — each service
terminal, or in Backoff with its dependents held for it (§6.1) — peinit
runs one more reload that applies what was deferred, recorded as
`peinit.config.reload.applied` naming the deferred services in
`graph.services`; nothing is emitted when nothing was deferred.
A service already running keeps its pinned definition as always.
<!-- lint-disable-next-line line-length -->
[*confgen.a-write-during-the-boot-window-is-deferred-for-an-unlaunched-service]

## The activation generation

When peinit starts a service, it snapshots that service's definition.
The snapshot governs the whole start lifecycle: pre-exec hooks, the
token request, the readiness timeout, the initial health checks. A field
changed while the service is Starting does not take effect until the
next start.
[*confgen.a-field-changed-while-starting-takes-effect-at-the-next-start]

A service in Inactive or Failed has no activation snapshot, so starting
it uses the current model.
[*confgen.an-inactive-service-starts-from-the-current-model] That gives
the expected behavior for the common edits:

- A new service entry is available once the change notification is
  processed.
- A timer change on an inactive service takes effect at the next
  trigger evaluation.
- A dependency change takes effect on the next start for an inactive
  service, and on the next restart for an active one.

## Field mutability

Which class a field falls into depends on when its value is consumed.

### Pinned to the running definition

A change takes effect only when the service is restarted:
[*confgen.the-pinned-fields]

`ImagePath`, `Type`, `Identity`, `RequiredPrivileges`, `ErrorControl`,
`RemainAfterExit`, `Triggers`, `Disabled`.

`Triggers` and `Disabled` are pinned only while the service is running.
On a service that is not running, both take effect as soon as the
notification is processed
<!-- lint-disable-next-line line-length -->
[*confgen.triggers-and-disabled-take-effect-at-once-on-a-service-that-is-not-running]
— which is what arms a timer added to an inactive service.

### Applied on the next start

A change takes effect at the next start or explicit graph reload, not
while services are running: [*confgen.the-next-start-fields]

`Requires`, `Wants`, `BindsTo`, `Conflicts`, `OnFailure`, `Conditions`,
`Asserts`.

### Reloaded at runtime

A change takes effect at the next relevant event, with no restart:
[*confgen.the-runtime-reloaded-fields]

`Arguments`, `SuccessExitCodes`, every timeout, and retry value
(`StartTimeout`, `StopTimeout`, `WatchdogTimeout`, `RestartDelay`,
`RestartMaxRetries`, `RestartWindow`, `PreStartCheckTimeout`),
`HealthCheck` and its three parameters, `RestartPolicy`, `Environment`,
`WorkingDirectory`, `ExecStartPre`, `ExecStartPost`, `ExecReload`,
`HookIdentity`, `Readiness`, `NotifyAccess`, `LimitNOFILE`,
`LimitCORE`, `FdStoreMax`, `TTYPath`, `TTYPrecedence`,
`RuntimeDirectories`, `Provides`, `TimerPersistent`, `TimerJitter`,
`SafeMode`, `DisplayName`, `Description`, and `ServiceSecurity`.

`ServiceSecurity` is the one whose reload is immediately observable:
a change takes effect on the very next control request against that
service (§4.6).
