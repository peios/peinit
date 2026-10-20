---
title: Losing the Registry
description: peinit depends on the registry once, hard, and then never the same way again — what losing it does in each phase and at runtime.
---

peinit depends on the registry once, hard, and then never again in the
same way. The difference between those two situations is most of what
this section is about.

## Phase

registryd failing to start, failing readiness, or failing the
schema-version probe sends peinit to recovery mode immediately. There is
no Phase 2 without a registry and nothing useful to degrade to. The
counter is incremented, so a persistently broken registryd burns boot
attempts even though every one of them fails the same way.
[*lostreg.a-phase-1-registryd-failure-still-increments-the-counter]

The recovery shell reached from *this* failure does not have registryd
running, and the offline tools (§2.8) are what an administrator has. A
Phase 1 failure earlier than the registryd step is different: recovery
starts a registryd of its own there (§2.8).

## Phase

A registry read that fails or times out during the definition read sends
peinit to recovery, for the same reason: there is no graph to boot.

## At runtime

This is where the design pays off. peinit holds a complete in-memory
model and does not read the registry during normal supervision, so
registryd going away does not stop peinit supervising anything.
[*lostreg.registryd-going-away-does-not-stop-supervision] Services keep
running, restarts keep working, the control socket keeps answering, and
timers keep firing.
[*lostreg.services-restarts-and-timers-keep-working-without-registryd]

What stops working is anything that needs new configuration:
reload-config fails. [*lostreg.reload-config-fails-without-registryd]
Change notifications stop arriving, and a timer's last-run timestamp
cannot be written — so a persistent timer can produce a spurious
catch-up on the next boot.
[*lostreg.a-timer-last-run-timestamp-cannot-be-written-without-registryd]

registryd itself is a Critical service
[*lostreg.registryd-is-a-critical-service], so its failure takes the
ordinary Critical path: restart budget, then sync and reboot. peinit
does not have to handle a permanently absent registryd at runtime,
because the system reboots first.

## A definition that will not decode

One definition that fails to decode fails that definition (§3.2). At
boot the key is marked Failed with cause `ValidationError` and every
other service starts normally; on a reload the key is failed the same
way and every other definition loads. One malformed key does not stop
unrelated definitions in the same batch from taking effect, and a boot
has no previous generation to fall back to anyway, so refusing everything
would mean not booting at all over a single malformed key — which is
what it used to do.
