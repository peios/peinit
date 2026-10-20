---
title: "peinit.service.abandoned"
description: "A service's processes survived SIGKILL and its post-kill timeout, so peinit gave up on them and moved the service to abandoned: its cgroup is still populated, unkillably."
---

A service's processes survived SIGKILL and its post-kill timeout, so peinit
gave up on them and moved the service to `abandoned`: its cgroup is still
populated, unkillably. Written only on the shutdown path so far; the
name also covers an abandonment anywhere else.

The event metadata is:

- **Event type:** `peinit.service.abandoned`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none
- **Cardinality:** once per service abandoned

Formerly `shutdown.abandoned`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `object.service.name` | `str` | required | The name of the service the event is about, as its definition names it. |
| `object.cgroup.path` | `str.path` | required | The cgroup the event is about, as its full path under `/sys/fs/cgroup/peinit`. |
| `object.service.state-previous` | `str.enum` | required | The service's state before the event, with the same values as `object.service.state`. |
| `object.service.state` | `str.enum` | required | Always `abandoned`. |
| `object.service.transition-cause` | `str.enum` | required | Why the service's state changed. |
| `object.service.generation` | `uint` | required | The service's activation generation: a counter peinit advances each time the service enters `starting`, so that every incarnation of a service has its own number. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
