---
title: "peinit.graph.operation.ended"
description: "An operation that is a member of a graph execution context reached its terminal outcome as the graph sees it: satisfied, so that what depends on it can proceed, or failed."
---

An operation that is a member of a graph execution context reached its
terminal outcome as the graph sees it: satisfied, so that what depends on
it can proceed, or failed. The operation's own `peinit.operation.ended`
records the same end from the operation's side.

The event metadata is:

- **Event type:** `peinit.graph.operation.ended`
- **Defined in:** `peinit.evman`
- **Tier:** verbose
- **Gating:** none — every member of a graph context is recorded
- **Cardinality:** one record per occurrence

Formerly `graph.operation_terminal`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `graph.context` | `uint` | required | The graph execution context an event belongs to: one boot's start of its service set, or one on-demand start of a service and everything it needs. |
| `object.operation.guid` | `bin.guid` | required | The peinit operation the event is about: a start, stop, restart, reload or reset of one service, from the moment it is requested to the moment it ends. |
| `object.service.name` | `str` | required | The name of the service the event is about, as its definition names it. |
| `outcome.success` | `bool` | required | True when the graph counts the member satisfied, false when it failed. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
