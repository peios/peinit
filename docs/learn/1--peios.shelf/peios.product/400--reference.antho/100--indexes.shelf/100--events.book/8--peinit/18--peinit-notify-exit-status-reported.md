---
title: "peinit.notify.exit-status.reported"
description: "A service's job sent EXIT_STATUS=, informationally; it is not the exit of any process."
---

A service's job sent `EXIT_STATUS=`, informationally; it is not the exit
of any process.

The event metadata is:

- **Event type:** `peinit.notify.exit-status.reported`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none — every authenticated `EXIT_STATUS=` is recorded
- **Cardinality:** one record per occurrence

Formerly `notify.exit_status`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `subject.service.name` | `str` | required | The service whose job sent a notification message. |
| `subject.job.guid` | `bin.guid` | required | The job whose process sent a notification message: the service's main process, or a submitted job. |
| `subject.job.activation-generation` | `uint` | required | The activation generation of the service whose job sent a notification message, as `object.job.activation-generation` describes it. |
| `subject.operation.guid` | `bin.guid` | optional | The peinit operation the sending job was serving when it sent a notification message, such as the start that a readiness message completes. |
| `notify.exit-status` | `int` | optional | Absent when the text sent was not a number. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
