---
title: "peinit.operation.ended"
description: "An operation reached a terminal state other than merged: it completed, failed, was canceled while still pending, or was aborted while running."
---

An operation reached a terminal state other than `merged`: it completed,
failed, was canceled while still pending, or was aborted while running.
One type for the four, because they are outcomes of one action (PGSS
<span>§</span>6.6); `object.operation.state` says which.

The event metadata is:

- **Event type:** `peinit.operation.ended`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none — every operation that ends, other than by merging, is
  recorded
- **Cardinality:** one record per occurrence

Formerly `operation.completed`, `operation.failed`, `operation.cancelled`,
and `operation.aborted`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `object.operation.guid` | `bin.guid` | required | The peinit operation the event is about: a start, stop, restart, reload or reset of one service, from the moment it is requested to the moment it ends. |
| `object.operation.type` | `str.enum` | required | What the operation does to its service. |
| `object.operation.source` | `str.enum` | required | Who or what asked for the operation. |
| `object.operation.state` | `str.enum` | required | `completed`, `failed`, `cancelled`,  or `aborted`. |
| `outcome.success` | `bool` | required | True exactly when `object.operation.state` is `completed`. |
| `outcome.detail` | `str` | optional | The operation's result as peinit words it for a client, such as `inactive` or `startup killed by shutdown`. Free text until it is given an enumeration. |
| `object.operation.duration` | `uint.duration` | optional | From the request, not from the start. Absent on a canceled operation, which never ran. |
| `object.service.name` | `str` | required | The name of the service the event is about, as its definition names it. |
| `subject.token.sid` | `bin.sid` | optional | The user SID of the effective token the operation ran under. |
| `subject.token.groups` | `bin.sid[]` | optional | The group SIDs carried by the effective token. |
| `subject.token.privileges` | `uint.flags` | optional | The privileges the effective token holds, as the flags of its 64-bit privilege word from `uapi/pkm/token.h`: bit n is the privilege whose LUID is n, so `SeDebugPrivilege` is bit 20. |
| `subject.token.privileges-enabled` | `uint.flags` | optional | Which of the privileges in `subject.token.privileges` are enabled, as flags in the same bit layout. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
