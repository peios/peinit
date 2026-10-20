---
title: "peinit.notify.stopping.reported"
description: "A service's job sent STOPPING=1, so peinit will not send it SIGTERM."
---

A service's job sent `STOPPING=1`, so peinit will not send it SIGTERM. The
record exists because the effect is an absence: without it, a service that
rightly got no SIGTERM looks the same afterward as one that wrongly did
not.

The event metadata is:

- **Event type:** `peinit.notify.stopping.reported`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none — every authenticated `STOPPING=1` is recorded
- **Cardinality:** one record per occurrence

Formerly `notify.stopping`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `subject.service.name` | `str` | required | The service whose job sent a notification message. |
| `subject.job.guid` | `bin.guid` | required | The job whose process sent a notification message: the service's main process, or a submitted job. |
| `subject.job.activation-generation` | `uint` | required | The activation generation of the service whose job sent a notification message, as `object.job.activation-generation` describes it. |
| `subject.operation.guid` | `bin.guid` | optional | The peinit operation the sending job was serving when it sent a notification message, such as the start that a readiness message completes. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
