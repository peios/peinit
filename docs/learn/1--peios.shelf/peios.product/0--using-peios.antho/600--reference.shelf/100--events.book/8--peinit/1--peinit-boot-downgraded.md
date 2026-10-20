---
title: "peinit.boot.downgraded"
description: "A full boot was downgraded to safe mode: the machine is running a reduced service set."
---

A full boot was downgraded to safe mode: the machine is running a reduced
service set. The services named are not marked failed, because safe mode
never meant to start them, so this record is the only one that names them
as the cause. Essential because it is rare and changes what the whole
machine is running.

The event metadata is:

- **Event type:** `peinit.boot.downgraded`
- **Defined in:** `peinit.evman`
- **Tier:** essential
- **Gating:** none
- **Cardinality:** one per finding that forced the downgrade, at most once a
  boot in all

Formerly `boot.safe_mode_downgrade`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `outcome.reason` | `str.enum` | required | The finding that forced the downgrade.<br><br>Values here (open set): `critical-cycle` · `critical-boot-conflict`. |
| `graph.services` | `str[]` | when `outcome.reason == critical-cycle` | The services a graph finding involves. |
| `object.service.name` | `str` | when `outcome.reason == critical-boot-conflict` | The name of the service the event is about, as its definition names it. |
| `object.service.conflict.name` | `str` | when `outcome.reason == critical-boot-conflict` | The name of the service this one conflicts with, on a finding that two services both triggered at boot declare a conflict and so cannot both start. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
