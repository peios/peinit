---
title: The Pre-Eventd Buffer
description: There is nowhere to send output until eventd binds its socket — what peinit buffers in memory, and why audit records are not logs.
---

Before eventd starts there is nowhere to send service output: eventd's
log socket does not exist until eventd binds it. peinit buffers in
memory until then. [*buffer.output-before-eventd-is-held-in-memory] The
buffer is bounded, and drops the **oldest** entries when it fills.
[*buffer.the-buffer-drops-the-oldest-entries]

| Key | Default | Minimum | Meaning |
|---|---|---|---|
| `Machine\System\Init\PreEventdBuffer` | 1048576 | 4096 | Bytes of output retained before eventd exists. [*buffer.the-pre-eventd-buffer-key] |

A value below the minimum keeps the default and logs a warning, rather
than being honored or failing the boot (§11.3). Zero is the case that
matters: a zero-capacity buffer rejects every record, so honoring it
would silently discard the whole pre-eventd window.

The value is read from the registry at boot and refreshed on reload, and
the buffer adopts the new capacity each time.
[*buffer.the-capacity-is-refreshed-on-reload] Lowering it takes effect
immediately, dropping the oldest entries until the contents fit — the
same end of the buffer a steady-state overrun drops.

Dropping the oldest rather than the newest is the right way round for
this buffer: the point of the window is the boot that is happening now,
and the most recent output is what explains where it got to.

> [!NOTE]
> In practice the only services that run before eventd are registryd,
> lpsd, authd and eudev. The first three are Peios-owned with controlled
> output. eudev is the real overrun risk, being verbose about device
> enumeration. A crash loop's output is bounded by the restart budget.

<!-- lint-disable-next-line line-length -->
## Audit records are not logs [*buffer.audit-records-go-to-kmes-not-the-buffer]

peinit's own audit records — access denials, critical failures, recovery
mode entry, graph errors, security-relevant transitions — are **events**
rather than logs. They go into the KMES kernel ring buffer, which
persists from the moment PKM loads: before eventd, before the registry,
before Phase 2.

So there is no pre-eventd buffer for them, and no window in which they
could be lost. eventd picks them up from the ring buffer when it
attaches, wherever in the boot they were emitted.
[*buffer.eventd-collects-earlier-events-when-it-attaches]

That distinction is the reason the two paths exist at all. Logs are
best-effort and lossy by design; audit events are neither.
