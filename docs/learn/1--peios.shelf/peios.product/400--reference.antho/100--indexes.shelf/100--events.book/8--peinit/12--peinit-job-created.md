---
title: "peinit.job.created"
description: "A job exists: peinit has decided to run a process and recorded it, before the process exists."
---

A job exists: peinit has decided to run a process and recorded it, before
the process exists. The gap to `peinit.job.started` is time spent queued,
and `peinit.job.ended` repeats everything here, which is why this one is
verbose.

The event metadata is:

- **Event type:** `peinit.job.created`
- **Defined in:** `peinit.evman`
- **Tier:** verbose
- **Gating:** none — every job peinit creates is recorded
- **Cardinality:** one record per occurrence

Formerly `job.created`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `object.job.guid` | `bin.guid` | required | The job the event is about. |
| `object.job.type` | `str.enum` | required | What the job is for. |
| `object.job.state` | `str.enum` | required | Always `created`. |
| `object.service.name` | `str` | when `object.job.type != submitted` | The name of the service the event is about, as its definition names it. |
| `object.job.activation-generation` | `uint` | when `object.job.type != submitted` | The activation generation of the service the job belongs to, at the time the job was created. |
| `object.operation.guid` | `bin.guid` | optional | The operation the job serves. Absent when none asked for it, as for a submitted job. |
| `object.job.executable` | `str.path` | required | The path of the executable the job runs. |
| `object.job.token.sid` | `bin.sid` | optional | A service's token is minted when its process is set up, after this record, so here it is the user the job is to run as when peinit knows that SID in advance — a well-known identity such as `SYSTEM` — and absent for an identity peinit has only by name. A submitted job's token exists from the start. |
| `object.job.token.groups` | `bin.sid[]` | optional | Present once peinit holds the job's token: a submitted job's at once, a service job's from `peinit.job.started`. |
| `object.job.token.privileges` | `uint.flags` | optional | Present exactly when `object.job.token.groups` is. |
| `object.job.token.privileges-enabled` | `uint.flags` | optional | Present exactly when `object.job.token.groups` is. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
