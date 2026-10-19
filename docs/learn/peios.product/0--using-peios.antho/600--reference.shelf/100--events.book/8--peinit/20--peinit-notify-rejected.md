---
title: "peinit.notify.rejected"
description: "peinit refused a datagram on the notification channel and applied none of it: the sender could not be authenticated, the message did not parse, or it made no sense for the job's state."
---

peinit refused a datagram on the notification channel and applied none of
it: the sender could not be authenticated, the message did not parse, or
it made no sense for the job's state. A stream of these from one process
is worth reading.

The event metadata is:

- **Event type:** `peinit.notify.rejected`
- **Defined in:** `peinit.evman`
- **Tier:** standard
- **Gating:** none — every refused notification datagram is recorded
- **Cardinality:** one record per occurrence

Formerly `notify.rejected`.

## Fields

The payload contains these fields:

| Field | Type | Presence | Meaning |
|---|---|---|---|
| `outcome.reason` | `str.enum` | required | Why the datagram was refused. `internal-error` is peinit unable to act on it, not anything wrong with the datagram; `shutdown-refused` is a timeout extension (`EXTEND_TIMEOUT_USEC=`) the shutdown in progress would not grant.<br><br>Values here (open set): `invalid-utf8` · `malformed-line` · `truncated` · `unauthenticated-sender` · `missing-service` · `job-not-running` · `missing-process` · `pidfd-mismatch` · `process-verification-failed` · `generation-mismatch` · `missing-start-operation` · `missing-reload-operation` · `unsupported-reload-operation` · `unsupported-ready-state` · `shutdown-refused` · `internal-error`. |
| `subject.process.pid` | `uint` | optional | The sending process, when the kernel gave its credentials. |
| `subject.service.name` | `str` | optional | The sender's service, when peinit could attribute it. |
| `subject.job.guid` | `bin.guid` | optional | The job whose process sent a notification message: the service's main process, or a submitted job. |
| `subject.job.activation-generation` | `uint` | optional | The activation generation of the service whose job sent a notification message, as `object.job.activation-generation` describes it. |
| `subject.operation.guid` | `bin.guid` | optional | The peinit operation the sending job was serving when it sent a notification message, such as the start that a readiness message completes. |

Every record also carries the header fields of [the
envelope](~kernel/internals/kmes/event-model), which no payload repeats.

The interface reference is `docs/peinit.evman` in the Peinit repository.
