---
title: "peinit.internal-error.contained"
description: "peinit could not carry out a step on a service's behalf, and contained the fault to that service."
---

peinit could not carry out a step on a service's behalf, and contained the
fault to that service. The fault is peinit's, not the service's; the job,
operation, and state change it caused have their own events.

The event metadata is:

- **Event type:** `peinit.internal-error.contained`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none — every contained internal error is recorded
- **Cardinality:** one record per occurrence

The error itself is not carried: every path that contains one has it only
as Rust Debug output, which no event field admits. peinit's console line
and the failed operation's result keep it.

Formerly `service.internal_error`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `operation.stage` | `str.enum` | required | The step peinit could not carry out.<br><br>Values here (open set): `job-terminal` · `process-setup` · `lifecycle-deadline` · `notify`. |
| `object.service.name` | `str` | optional | The name of the service the event is about, as its definition names it. |
| `object.job.guid` | `bin.guid` | optional | The job the event is about. |
| `object.service.failed` | `bool` | optional | Present exactly when `object.service.name` is. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
