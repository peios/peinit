---
title: "peinit.service.reload.timed-out"
description: "A service announced RELOADING=1 and never confirmed the reload by its deadline: it wedged mid-reload, or lost its handler."
---

A service announced `RELOADING=1` and never confirmed the reload by its
deadline: it wedged mid-reload, or lost its handler. Recorded because the
default `reload` does not wait, so nothing else would say so.

The event metadata is:

- **Event type:** `peinit.service.reload.timed-out`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none — every reload left unconfirmed past its deadline is
  recorded
- **Cardinality:** one record per occurrence

Formerly `service.reload_unconfirmed`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `object.service.name` | `str` | required | The name of the service the event is about, as its definition names it. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
