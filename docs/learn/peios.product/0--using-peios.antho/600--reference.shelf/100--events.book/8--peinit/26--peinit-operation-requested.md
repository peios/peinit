---
title: "peinit.operation.requested"
description: "An operation on a service — a start, stop, restart, reload,  or reset — was created."
---

An operation on a service — a start, stop, restart, reload, or reset — was
created. A client's command creates one with `object.operation.source`
`admin`; every other source is peinit acting for its own reasons. Every
operation in a graph context is requested when the context is built.

The event metadata is:

- **Event type:** `peinit.operation.requested`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none — every operation is recorded
- **Cardinality:** one record per occurrence

Formerly `operation.requested`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `object.operation.guid` | `bin.guid` | required | The peinit operation the event is about: a start, stop, restart, reload or reset of one service, from the moment it is requested to the moment it ends. |
| `object.operation.type` | `str.enum` | required | What the operation does to its service. |
| `object.operation.source` | `str.enum` | required | Who or what asked for the operation. |
| `object.operation.state` | `str.enum` | required | Always `pending`. |
| `object.service.name` | `str` | required | The name of the service the event is about, as its definition names it. |
| `subject.token.sid` | `bin.sid` | optional | The client whose command created the operation. Absent for an operation peinit created itself. |
| `subject.token.groups` | `bin.sid[]` | optional | Written only when peinit holds the client's whole token. For a control-channel caller it holds the user SID alone, so today an operation's subject is `subject.token.sid` and nothing more. |
| `subject.token.privileges` | `uint.flags` | optional | Present exactly when `subject.token.groups` is. |
| `subject.token.privileges-enabled` | `uint.flags` | optional | Present exactly when `subject.token.groups` is. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
