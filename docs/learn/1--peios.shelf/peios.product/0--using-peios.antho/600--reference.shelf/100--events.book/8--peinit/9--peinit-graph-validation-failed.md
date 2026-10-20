---
title: "peinit.graph.validation.failed"
description: "The service graph failed validation for the reason in outcome.reason."
---

The service graph failed validation for the reason in `outcome.reason`. At
`boot` the services named are marked and the boot continues; at
`reload-config` the whole reload is rejected; at `phase2-boot` the boot
plan could not be built.

The event metadata is:

- **Event type:** `peinit.graph.validation.failed`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none — every finding is recorded
- **Cardinality:** one per finding; a validation with several findings writes
  several

Formerly `graph.validation_error`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `graph.phase` | `str.enum` | required | When the service graph was validated. |
| `outcome.reason` | `str.enum` | required | The finding.<br><br>Values here (open set): `invalid-service-name` · `duplicate-service` · `missing-service-definition` · `missing-hard-dependency` · `hard-dependency-blocked` · `cycle` · `conflicting-boot-services` · `invalid-health-check-restart-window` · `unschedulable-health-check` · `invalid-timer-schedule` · `validation-error`. |
| `object.service.name` | `str` | optional | The service the finding is about. Absent on `cycle`, which names several. |
| `graph.services` | `str[]` | when `outcome.reason == cycle` | The services a graph finding involves. |
| `object.service.dependency.name` | `str` | optional | On `missing-hard-dependency` and `hard-dependency-blocked`. |
| `object.service.dependency.kind` | `str.enum` | optional | Present exactly when `object.service.dependency.name` is. |
| `object.service.conflict.name` | `str` | when `outcome.reason == conflicting-boot-services` | The name of the service this one conflicts with, on a finding that two services both triggered at boot declare a conflict and so cannot both start. |
| `object.service.health-check.interval` | `uint.duration` | when `outcome.reason == invalid-health-check-restart-window` | The configured interval between health checks, in nanoseconds. |
| `object.service.health-check.retries` | `uint` | when `outcome.reason == invalid-health-check-restart-window` | The configured number of consecutive health-check failures at which peinit stops reporting the service unhealthy and acts on it. |
| `object.service.health-check.restart-window` | `uint.duration` | when `outcome.reason == invalid-health-check-restart-window` | The configured restart window of a health check, in nanoseconds. |
| `object.service.type` | `str.enum` | when `outcome.reason == unschedulable-health-check` | How the service's main process is supervised. |
| `object.service.timer.schedule` | `str` | when `outcome.reason == invalid-timer-schedule` | The service's timer schedule, exactly as its definition wrote it. |
| `outcome.detail` | `str` | optional | On `invalid-timer-schedule`, the parser's message; on `validation-error`, why the definition was refused. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
