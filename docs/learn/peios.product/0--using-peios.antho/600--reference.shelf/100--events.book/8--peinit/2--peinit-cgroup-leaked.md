---
title: "peinit.cgroup.leaked"
description: "A cgroup could not be reclaimed: its processes do not respond to the kernel, and the cgroup is leaked."
---

A cgroup could not be reclaimed: its processes do not respond to the
kernel, and the cgroup is leaked. It is one of a service's sub-cgroups, or
the whole cgroup of a submitted job.

The event metadata is:

- **Event type:** `peinit.cgroup.leaked`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none
- **Cardinality:** once per cgroup, the first time it is found still populated
  after its post-kill deadline

Formerly `cgroup.leaked`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `object.service.name` | `str` | optional | The service whose cgroup leaked. Absent for a submitted job's cgroup, which belongs to no service. |
| `object.job.guid` | `bin.guid` | optional | The submitted job whose cgroup leaked. Exactly one of `object.service.name` and this field is present. |
| `object.cgroup.path` | `str.path` | required | The cgroup the event is about, as its full path under `/sys/fs/cgroup/peinit`. |
| `object.cgroup.type` | `str.enum` | required | `service-tree` for a submitted job's cgroup, which is the whole of its containment. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
