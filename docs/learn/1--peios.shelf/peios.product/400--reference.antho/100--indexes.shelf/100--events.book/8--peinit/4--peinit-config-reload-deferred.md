---
title: "peinit.config.reload.deferred"
description: "A configuration reload during the boot window changed definitions of services the boot plan had not yet launched."
---

A configuration reload during the boot window changed definitions of
services the boot plan had not yet launched. The boot runs against its
snapshot, so those services start from the plan, and the change waits for
`peinit.config.reload.applied`.

The event metadata is:

- **Event type:** `peinit.config.reload.deferred`
- **Defined in:** `peinit.evman`
- **Tier:** verbose
- **Gating:** none
- **Cardinality:** once per reload during the boot window that left
  definitions pending

Formerly `config.reload_deferred`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `graph.services` | `str[]` | required | The services whose definitions were deferred. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
