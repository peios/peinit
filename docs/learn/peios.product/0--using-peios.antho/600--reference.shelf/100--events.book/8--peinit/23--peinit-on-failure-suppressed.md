---
title: "peinit.on-failure.suppressed"
description: "peinit declined to start an OnFailure handler, because starting it would have looped or gone too deep, so the failure it was meant to handle went unhandled."
---

peinit declined to start an OnFailure handler, because starting it would
have looped or gone too deep, so the failure it was meant to handle went
unhandled.

The event metadata is:

- **Event type:** `peinit.on-failure.suppressed`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none — every suppressed handler is recorded
- **Cardinality:** one record per occurrence

Formerly `on_failure.loop_suppressed`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `object.service.name` | `str` | required | The service whose failure the chain was handling. |
| `object.service.on-failure.name` | `str` | required | The OnFailure handler peinit declined to start, because starting it would have looped or gone too deep. |
| `object.service.on-failure-chain` | `str[]` | required | The OnFailure handlers already started in the chain that peinit cut short, in the order they were started. |
| `outcome.reason` | `str.enum` | required | Why the operation resolved the way it did.<br><br>Values here (closed set): `cycle` · `max-depth`. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
