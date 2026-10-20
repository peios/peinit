---
title: "peinit.graph.validation.warned"
description: "The service graph passed validation with a warning: nothing was refused, but something will not behave as its author probably expects."
---

The service graph passed validation with a warning: nothing was refused,
but something will not behave as its author probably expects.

The event metadata is:

- **Event type:** `peinit.graph.validation.warned`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none — every warning is recorded
- **Cardinality:** one per warning

Formerly `graph.validation_warning`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `graph.phase` | `str.enum` | required | When the service graph was validated. |
| `outcome.reason` | `str.enum` | required | The warning.<br><br>Values here (open set): `alive-readiness-with-hard-dependents` · `unfilled-role`. |
| `object.service.name` | `str` | when `outcome.reason == alive-readiness-with-hard-dependents` | The name of the service the event is about, as its definition names it. |
| `object.service.dependents` | `str[]` | when `outcome.reason == alive-readiness-with-hard-dependents` | The services that hard-depend on this one, by name. |
| `graph.role` | `str` | when `outcome.reason == unfilled-role` | The role no service fills. |
| `graph.services` | `str[]` | when `outcome.reason == unfilled-role` | The services a graph finding involves. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
