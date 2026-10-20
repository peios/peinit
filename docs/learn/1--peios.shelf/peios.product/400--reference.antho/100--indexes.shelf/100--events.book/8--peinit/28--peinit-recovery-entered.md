---
title: "peinit.recovery.entered"
description: "peinit dropped the machine to a recovery shell, for the reason in outcome.reason."
---

peinit dropped the machine to a recovery shell, for the reason in
`outcome.reason`. Written before the registry or eventd can exist, into the
ring, which is where the record of a boot that failed this early survives.
Essential because it is rare and means the machine did not boot.

The event metadata is:

- **Event type:** `peinit.recovery.entered`
- **Defined in:** `peinit.evman`
- **Tier:** essential
- **Gating:** none
- **Cardinality:** once, when peinit gives up on the boot and starts the
  recovery shell

Formerly `recovery.entered`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `outcome.reason` | `str.enum` | required | Why the operation resolved the way it did.<br><br>Values here (open set): `kernel-command-line` · `privileges` · `boot-attempt-counter` · `forced-by-kernel-command-line` · `boot-attempt-threshold-reached` · `root-writable` · `virtual-filesystems` · `machine-id` · `rtc-clock` · `registryd` · `provisioning` · `infrastructure` · `phase2` · `runtime`. |
| `outcome.detail` | `str` | optional | What went wrong, in the error's own words, such as `peinit is missing required privilege(s): SeAuditPrivilege`. Absent for `forced-by-kernel-command-line`, which has nothing to add, and for `phase2`, whose findings are their own `peinit.graph.validation.failed` records. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
