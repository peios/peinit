---
title: "peinit.operation.merged"
description: "An operation was absorbed into another before it ran, and has no outcome of its own: follow object.operation.merged-into.guid to the operation whose peinit.operation.ended decides it."
---

An operation was absorbed into another before it ran, and has no outcome
of its own: follow `object.operation.merged-into.guid` to the operation
whose `peinit.operation.ended` decides it.

The event metadata is:

- **Event type:** `peinit.operation.merged`
- **Defined in:** `peinit.evman`
- **Tier:** verbose
- **Gating:** none — every merge is recorded
- **Cardinality:** one record per occurrence

Formerly `operation.merged`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `object.operation.guid` | `bin.guid` | required | The peinit operation the event is about: a start, stop, restart, reload or reset of one service, from the moment it is requested to the moment it ends. |
| `object.operation.type` | `str.enum` | required | What the operation does to its service. |
| `object.operation.source` | `str.enum` | required | Who or what asked for the operation. |
| `object.operation.state` | `str.enum` | required | Always `merged`. |
| `object.operation.merged-into.guid` | `bin.guid` | required | The operation that absorbed this one. |
| `object.service.name` | `str` | required | The name of the service the event is about, as its definition names it. |
| `subject.token.sid` | `bin.sid` | optional | The user SID of the effective token the operation ran under. |
| `subject.token.groups` | `bin.sid[]` | optional | The group SIDs carried by the effective token. |
| `subject.token.privileges` | `uint.flags` | optional | The privileges the effective token holds, as the flags of its 64-bit privilege word from `uapi/pkm/token.h`: bit n is the privilege whose LUID is n, so `SeDebugPrivilege` is bit 20. |
| `subject.token.privileges-enabled` | `uint.flags` | optional | Which of the privileges in `subject.token.privileges` are enabled, as flags in the same bit layout. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
