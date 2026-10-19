---
title: "peinit.job.ended"
description: "A job is over: its process exited or was killed, or the job ended without its process ever starting."
---

A job is over: its process exited or was killed, or the job ended without
its process ever starting. It carries the whole job record, so it is the
one job event to read when only one is kept.

The event metadata is:

- **Event type:** `peinit.job.ended`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none — every job that ends is recorded
- **Cardinality:** one record per occurrence

Formerly `job.ended`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `object.job.guid` | `bin.guid` | required | The job the event is about. |
| `object.job.type` | `str.enum` | required | What the job is for. |
| `object.job.state` | `str.enum` | required | The terminal state: `completed`, `failed`,  or `abandoned`. |
| `outcome.success` | `bool` | required | True exactly when `object.job.state` is `completed`. |
| `outcome.detail` | `str` | optional | Why the job did not complete, in peinit's words, such as `ProcessCrash: signal 9`. Present only when `outcome.success` is false and peinit recorded a cause. Free text until it is given an enumeration. |
| `object.service.name` | `str` | when `object.job.type != submitted` | The name of the service the event is about, as its definition names it. |
| `object.job.activation-generation` | `uint` | when `object.job.type != submitted` | The activation generation of the service the job belongs to, at the time the job was created. |
| `object.operation.guid` | `bin.guid` | optional | The peinit operation the event is about: a start, stop, restart, reload or reset of one service, from the moment it is requested to the moment it ends. |
| `object.process.pid` | `uint` | optional | Absent when the process never started. |
| `object.process.exit-code` | `int` | optional | The exit code of the job's process, as it passed it to `exit`. |
| `object.process.exit-signal` | `int` | optional | The number of the signal that ended the job's process. |
| `object.job.executable` | `str.path` | required | The path of the executable the job runs. |
| `object.job.arguments` | `str[]` | required | The job's argument vector, including `argv[0]`. |
| `object.job.arguments-truncated` | `bool` | required | Whether `object.job.arguments` was cut to fit the record. |
| `object.job.arguments-count` | `uint` | required | How many arguments the job was given, before any were cut. |
| `object.job.created-time` | `uint.time` | required | When peinit created the job, in nanoseconds since the Unix epoch. |
| `object.job.started-time` | `uint.time` | optional | When the job's process started, in nanoseconds since the Unix epoch. |
| `object.job.duration` | `uint.duration` | required | How long the job lasted, in nanoseconds, from its creation to its end. |
| `object.cgroup.path` | `str.path` | required | The cgroup the event is about, as its full path under `/sys/fs/cgroup/peinit`. |
| `object.cgroup.generation` | `uint` | required | The cgroup generation of the service the cgroup belongs to. |
| `object.job.token.sid` | `bin.sid` | optional | As on `peinit.job.started`, for a job whose process started. A service's job that ended without starting had no token yet, and is as on `peinit.job.created`. |
| `object.job.token.groups` | `bin.sid[]` | optional | Present exactly when the job's process started, or it is a submitted job. |
| `object.job.token.privileges` | `uint.flags` | optional | Present exactly when `object.job.token.groups` is. |
| `object.job.token.privileges-enabled` | `uint.flags` | optional | Present exactly when `object.job.token.groups` is. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
