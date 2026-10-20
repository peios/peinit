---
title: "peinit.job.output.dropped"
description: "A submitted job's submitter stopped draining its output, and peinit began dropping the submitter's copy of the job's lines."
---

A submitted job's submitter stopped draining its output, and peinit began
dropping the submitter's copy of the job's lines. The copy sent to the log
store is unaffected.

The event metadata is:

- **Event type:** `peinit.job.output.dropped`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none
- **Cardinality:** once per job, on the first line dropped

Formerly `output.dropped`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `object.job.guid` | `bin.guid` | required | The job the event is about. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
