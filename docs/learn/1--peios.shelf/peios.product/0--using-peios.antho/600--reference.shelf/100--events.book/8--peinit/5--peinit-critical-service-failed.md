---
title: "peinit.critical-service.failed"
description: "A service marked Critical failed, and peinit took the machine to its reboot final action."
---

A service marked Critical failed, and peinit took the machine to its
reboot final action. Essential because it is rare and reboots the machine.

The event metadata is:

- **Event type:** `peinit.critical-service.failed`
- **Defined in:** `peinit.evman`
- **Tier:** essential
- **Gating:** none
- **Cardinality:** once per Critical failure that drives the machine to reboot

Formerly `critical.failure`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `object.service.name` | `str` | required | The name of the service the event is about, as its definition names it. |
| `outcome.reason` | `str.enum` | required | How the Critical service failed.<br><br>Values here (open set): `service-main-terminal` · `health-check-failure` · `watchdog-timeout` · `restart-budget-exhausted`. |
| `shutdown.finalization-state` | `str.enum` | required | How far the system shutdown had got when the event was written. |
| `outcome.detail` | `str` | when `shutdown.finalization-state == failed` | Why finalization failed. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
