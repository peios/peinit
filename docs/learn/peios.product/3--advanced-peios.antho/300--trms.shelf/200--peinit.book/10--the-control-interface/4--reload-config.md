---
title: Reload-config
description: A full atomic re-read of the registry rather than a live update — what changes, and how removals and the compiled-in service are handled.
---

`reload-config` takes a fresh snapshot of the configuration from the
registry. It does not live-update anything.

It is also the path a registry change notification takes: any drained
watch event triggers the same full reload, rather than a targeted
re-read of whatever changed.
[*control.reload-config.is-the-registry-watch-path]

## Atomicity

peinit reads everything first. Every registry read happens before any
mutation, so a read failure returns an error with nothing touched.
[*control.reload-config.reads-precede-every-mutation] It
then builds and validates a complete new graph in memory, and only swaps
it in if validation succeeds.

If validation fails, the previous generation stays live and the findings
are returned to the caller.
[*control.reload-config.a-failed-validation-changes-nothing] This is
where the reload path differs
sharply from boot: boot marks individual services Failed and carries on,
because it has to produce a running system; reload rejects the whole
thing, because it has a running system already and a half-applied
configuration would be worse than the one in place.

An explicit `reload-config` writes one `peinit.config.reload.applied`
event either way: `outcome.success`, and on success the counts of what
it added, updated, restored, marked removed, discarded and found
undecodable; on failure why, in words, and one
`peinit.graph.validation.failed` per finding under phase
`reload-config` (§8.4). A reload a registry watch starts writes its
findings and warnings but no `peinit.config.reload.applied`.

Decoding does not split: reload decodes per key exactly as boot does. A
key that will not decode fails that one service with `ValidationError` —
a placeholder entry, marked definition-removed, that `status` reports
and nothing can start — and every other definition in the batch loads; a
dependent of the failed key fails through ordinary propagation when it
is next started. A service that is running when its key stops decoding
keeps running with its definition marked removed, as if the key had been
deleted. The reload succeeds, lists the keys in `summary.undecodable`,
and names each key, the offending field and the problem in
`undecodable`; each is also a `peinit.graph.validation.failed` event
with `outcome.reason` `validation-error` under phase `reload-config`. A
key whose name is not a service name (§3.1) is
undecodable in the same way, reported under its raw name with `field:
name`. Repairing the key restores the service on the next reload.
[*control.reload-config.an-undecodable-definition-fails-only-that-service]

## What changes

- Every service definition is re-read.
  [*control.reload-config.every-definition-is-re-read]
- A new dependency graph is built and validated.
- Running services are unaffected and continue on their activation
  generation. [*control.reload-config.running-services-are-unaffected]
- New definitions take effect at the next start, restart, or trigger.
  [*control.reload-config.a-changed-definition-takes-effect-at-the-next-start]
- New services become available for `start` immediately.
  [*control.reload-config.a-new-service-is-startable-at-once]
- Timer triggers are re-evaluated. Unchanged calendar timers retain their
  armed occurrence and jittered deadline (§9.3); new or changed timers
  are armed from the current time with no catch-up.
  [*control.reload-config.calendar-timers-are-re-armed-without-catch-up]

A reload also refreshes things that are not service definitions: the
control descriptor, the four control socket limits, the log
configuration, the shutdown settings, the global environment layer, and
the eventd log socket path.
[*control.reload-config.refreshes-more-than-definitions] It also prunes
the fd stores of services that have been removed.
[*control.reload-config.prunes-fd-stores-of-vanished-services]

## Removals and the compiled-in service

A definition that has disappeared is handled by §3.8 — discarded if
nothing is running, marked definition-removed if something is.

registryd is exempt. Its compiled-in definition survives a reload that
does not mention it, and its provenance survives a registry entry that
shadows it. [*control.reload-config.registryd-is-exempt] peinit started
registryd before the registry existed, and a
reload finding no definition for it cannot conclude that it should stop
being managed.
