---
title: "peinit.job.started"
description: "A job's process exists and has been executed: the moment a process is running as object.job.token.sid on peinit's behalf."
---

A job's process exists and has been executed: the moment a process is
running as `object.job.token.sid` on peinit's behalf.

The event metadata is:

- **Event type:** `peinit.job.started`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none — every job whose process starts is recorded
- **Cardinality:** one record per occurrence

Formerly `job.started`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `object.job.guid` | `bin.guid` | required | The job the event is about. |
| `object.job.type` | `str.enum` | required | What the job is for. |
| `object.job.state` | `str.enum` | required | Always `running`. |
| `object.service.name` | `str` | when `object.job.type != submitted` | The name of the service the event is about, as its definition names it. |
| `object.job.activation-generation` | `uint` | when `object.job.type != submitted` | The activation generation of the service the job belongs to, at the time the job was created. |
| `object.operation.guid` | `bin.guid` | optional | The peinit operation the event is about: a start, stop, restart, reload or reset of one service, from the moment it is requested to the moment it ends. |
| `object.process.pid` | `uint` | required | The job's process. |
| `object.cgroup.path` | `str.path` | required | The cgroup the job's process runs in. |
| `object.job.token.sid` | `bin.sid` | required | The user SID of the token the job's process runs as. |
| `object.job.token.groups` | `bin.sid[]` | required | The group SIDs of the token the job's process runs as, in the token's order. |
| `object.job.token.privileges` | `uint.flags` | required | The privileges present on the token the job's process runs as, as the flags of the 64-bit privilege word of `uapi/pkm/token.h`, one `KACS_SE_*_PRIVILEGE` bit each. |
| `object.job.token.privileges-enabled` | `uint.flags` | required | Which of the privileges in `object.job.token.privileges` are enabled, as flags in the same 64-bit word. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
