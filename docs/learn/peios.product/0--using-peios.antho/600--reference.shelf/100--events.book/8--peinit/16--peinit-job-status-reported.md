---
title: "peinit.job.status.reported"
description: "A submitted job reported STATUS= or PROGRESS= on the notification channel, and this is what peinit retained after it."
---

A submitted job reported `STATUS=` or `PROGRESS=` on the notification
channel, and this is what peinit retained after it.

The event metadata is:

- **Event type:** `peinit.job.status.reported`
- **Defined in:** `peinit.evman`
- **Tier:** verbose
- **Gating:** none — every change a submitted job reports is recorded, within
  the rate bound
- **Cardinality:** at most one per job per second; a report inside the window
  updates the job view and writes nothing

Formerly `job.status`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `object.job.guid` | `bin.guid` | required | The job the event is about. |
| `object.job.submitter.sid` | `bin.sid` | required | The user SID of the client that submitted the job. |
| `notify.status` | `str` | optional | The text a service sent as `STATUS=`: free text describing what it is doing. |
| `notify.progress.current` | `uint` | optional | How far a service or submitted job has got, as it reported with `PROGRESS=`: the count before any `/`. |
| `notify.progress.total` | `uint` | optional | The end `notify.progress.current` is counting toward, as reported after the `/` of `PROGRESS=`. |
| `notify.progress.bounded` | `bool` | optional | Whether the progress a service or submitted job reported declares an end: true for `PROGRESS=N/` and `PROGRESS=N/M`, false for a bare `PROGRESS=N`, which counts with no end (PSPU <span>§</span>4.19). |
| `notify.progress.unit` | `str.enum` | optional | What `notify.progress.current` and `notify.progress.total` count, as reported with `PROGRESS_UNIT=`. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
