---
title: "peinit.notify.status.reported"
description: "A service's job sent STATUS= on the notification channel."
---

A service's job sent `STATUS=` on the notification channel. The text is the
service's own words, not peinit's. Not rate-bounded: a service decides how
often it sends one.

The event metadata is:

- **Event type:** `peinit.notify.status.reported`
- **Defined in:** `peinit.evman`
- **Tier:** verbose
- **Gating:** none — every authenticated `STATUS=` is recorded
- **Cardinality:** one record per occurrence

Formerly `notify.status`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `subject.service.name` | `str` | required | The service whose job sent a notification message. |
| `subject.job.guid` | `bin.guid` | required | The job whose process sent a notification message: the service's main process, or a submitted job. |
| `subject.job.activation-generation` | `uint` | required | The activation generation of the service whose job sent a notification message, as `object.job.activation-generation` describes it. |
| `subject.operation.guid` | `bin.guid` | optional | The peinit operation the sending job was serving when it sent a notification message, such as the start that a readiness message completes. |
| `notify.status` | `str` | required | The text a service sent as `STATUS=`: free text describing what it is doing. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
