---
title: "peinit.notify.errno.reported"
description: "A service's job sent ERRNO=: its own claim about an error, which peinit neither acts on nor retains."
---

A service's job sent `ERRNO=`: its own claim about an error, which peinit
neither acts on nor retains.

The event metadata is:

- **Event type:** `peinit.notify.errno.reported`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none — every authenticated `ERRNO=` is recorded
- **Cardinality:** one record per occurrence

Formerly `notify.errno`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `subject.service.name` | `str` | required | The service whose job sent a notification message. |
| `subject.job.guid` | `bin.guid` | required | The job whose process sent a notification message: the service's main process, or a submitted job. |
| `subject.job.activation-generation` | `uint` | required | The activation generation of the service whose job sent a notification message, as `object.job.activation-generation` describes it. |
| `subject.operation.guid` | `bin.guid` | optional | The peinit operation the sending job was serving when it sent a notification message, such as the start that a readiness message completes. |
| `notify.errno` | `int.errno` | optional | Absent when the text sent was not a positive number. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
