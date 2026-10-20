---
title: "peinit.fd-store.rejected"
description: "peinit refused a file descriptor a service asked it to keep."
---

peinit refused a file descriptor a service asked it to keep.

The event metadata is:

- **Event type:** `peinit.fd-store.rejected`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none — every refused descriptor is recorded
- **Cardinality:** one record per occurrence

Formerly `fd_store.rejected`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `object.service.name` | `str` | required | The name of the service the event is about, as its definition names it. |
| `object.fd-store.name` | `str` | required | The name a service gave a file descriptor it asked peinit to keep, with `FDNAME=`. |
| `outcome.reason` | `str.enum` | required | `disabled` when the service's definition does not enable an fd store; `full` when it is at its limit.<br><br>Values here (open set): `disabled` · `full`. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
