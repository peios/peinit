---
title: "peinit.config.reload.applied"
description: "A configuration reload ran: from now on the registry as it then stood is the running configuration, or, if it failed, nothing changed."
---

A configuration reload ran: from now on the registry as it then stood is
the running configuration, or, if it failed, nothing changed. Written for
an explicit `reload-config`, and for the one reload that applies what the
boot window deferred. A reload a registry watch starts writes none.

The event metadata is:

- **Event type:** `peinit.config.reload.applied`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none
- **Cardinality:** once per explicit `reload-config`, and once a boot at most
  when the boot window closes with reloads deferred

A failed validation also writes one `peinit.graph.validation.failed` per
finding, under the `reload-config` phase.

Formerly `config.reload_coalesced`, which an explicit reload did not write.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `outcome.success` | `bool` | required | Whether the operation succeeded. |
| `outcome.detail` | `str` | optional | Why the reload failed, for a person. Present only when `outcome.success` is false. |
| `graph.services` | `str[]` | optional | The services whose definitions the boot window had deferred. Present exactly on the reload that applies them, which is how it is told from an explicit `reload-config`. |
| `graph.counts.added` | `uint` | when `outcome.success == true` | How many service definitions a configuration reload added: names that had no definition before. |
| `graph.counts.updated` | `uint` | when `outcome.success == true` | How many existing service definitions a configuration reload changed. |
| `graph.counts.restored` | `uint` | when `outcome.success == true` | How many definitions a configuration reload brought back after an earlier reload had removed them while their services were still running. |
| `graph.counts.marked-removed` | `uint` | when `outcome.success == true` | How many definitions a configuration reload withdrew from services that were still running. |
| `graph.counts.discarded` | `uint` | when `outcome.success == true` | How many definitions a configuration reload withdrew and dropped at once, because their services were not running. |
| `graph.counts.undecodable` | `uint` | when `outcome.success == true` | How many definitions a configuration reload found in the registry but could not decode. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
