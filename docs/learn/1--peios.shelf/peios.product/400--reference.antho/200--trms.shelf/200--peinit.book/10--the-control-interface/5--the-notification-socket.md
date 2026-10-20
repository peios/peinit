---
title: The Notification Socket
description: The datagram socket services report on themselves over — authenticating a sender, applying a datagram, and the bounds on it.
---

peinit binds one Unix datagram socket for service notifications, by
default at `/run/services/peinit/notify.sock`. Its path is an
implementation detail: services receive it through `NOTIFY_SOCKET` and
nothing hardcodes it.
[*notify.the-path-reaches-a-service-through-notify-socket]
The kernel command line can override it with `peios.notifysocket=`.
[*notify.the-path-is-overridable-on-the-kernel-command-line]
peinit creates the socket's directory only when it lies under `/run`;
an overriding path anywhere else must have a parent that already
exists. `/run/services/peinit` itself is created whichever path is in
use.

There is one socket, not one per service, and the bind unlinks any stale
path first. [*notify.there-is-one-datagram-socket-for-every-service]

## The descriptor [*notify.the-socket-descriptor]

```text
O:SYG:SYD:(A;;GA;;;SY)(A;;FW;;;S-1-5-6)
```

`S-1-5-6` is the Service group, which every token minted for a service
logon carries.

It is written out rather than as its `SU` alias, and that is deliberate
rather than an oversight. The alias table lives in libpeios, which is a
separately versioned package that nothing here declares a minimum
version of — so an image pairing a peinit that writes `SU` with a
libpeios that predates the alias parses this descriptor, fails, and
takes PID 1 into recovery before a console exists to say why. That is
not hypothetical: it is what happened the first time this descriptor was
written. A literal SID has been understood by every version there has
ever been. That is the right grantee because it *is* the
population with something to say here: membership follows from having
been started as a service rather than from which account a process runs
under, so an ordinary user process cannot acquire it however it was
launched.

`FW` is the file-write generic right, which is what a write to a
pathname socket needs — the same reasoning as the jobs socket (§10.7).
Administrators are absent, unlike the control socket: an administrator
has no business asserting that a service is ready.

The descriptor is installed **by path, immediately after `bind` ** —
before the socket is made non-blocking, given `SO_PASSCRED`, or polled.

It is by path because the fd form does not work on a socket. KACS
exposes no fd-targeted descriptor call of its own: `fd_set_sd` is the
path call with the target as `dirfd`, an empty path and `AT_EMPTY_PATH`,
and the kernel answers `EOPNOTSUPP` for a socket. Stamping by fd was
tried here and took PID 1 into recovery.

That leaves a window between `bind` and the stamp in which the socket
carries what it inherited. It is safe for the same reason it is on the
control socket: the parent grants services traverse and nothing else and
carries no inheritable ACE, so the inherited descriptor is narrower than
the one being installed. The failure direction is refusal, not
exposure.

The parent directory `/run/services/peinit` carries a descriptor of its
own, shared with the control socket that lands in it later in boot:
SYSTEM and Administrators in full, and `S-1-5-6` traverse only.
[*notify.the-parent-directory-descriptor] One constant
serves both because `ensure_directory` re-stamps a directory that
already exists — two call sites installing different descriptors on one
path would silently leave whichever ran last, and since the notify
socket is bound first, that would be the control socket's, quietly
removing the traverse services need to reach `notify.sock` at all.

The protocol itself is PSPU §4. What follows is how peinit decides
whether to believe a datagram.

## Authenticating a sender

`SO_PASSCRED` is enabled on the socket, so every datagram arrives with a
kernel-attested `SCM_CREDENTIALS` control message. A datagram without one
is rejected outright. [*notify.a-datagram-without-credentials-is-rejected]
Descriptors for the fd store arrive alongside, as `SCM_RIGHTS`.

Authentication then runs five steps, and each closes a hole the previous
one leaves:

1. **Find the sender.** Scan for the current *service-main* job whose
   PID equals the sender's. Only a main job is ever a candidate, which
   is what makes `NotifyAccess=Main` the only mode there is — a hook or
   a health check cannot notify on a service's behalf.
<!-- lint-disable-next-line spelling -->
   [*notify.only-a-services-main-job-may-notify] If no service's
   main job carries the PID, the live submitted jobs (§8.5) are
   scanned instead. The routing question is asked purely by PID, before
   either door verifies anything, so that a sender is verified against
   its pidfd exactly once whichever way it is routed.
2. **The job is Running.** A job still in pending setup has not exec'd.
3. **The job has a pidfd.**
4. **The pidfd still refers to that PID.** This is the step that
   matters: PID matching alone is racy, because a PID can be recycled
   between the sender writing and peinit reading. The pidfd was obtained
   atomically at fork, so verifying the PID against it is what makes the
   match sound rather than probable.
   [*notify.the-pidfd-is-verified-against-the-senders-pid]
5. **The generation matches.** A job whose activation generation is
   not the service's current one is a previous incarnation, and its
   notifications are rejected.
   [*notify.a-stale-activation-generation-is-rejected] This is
   invariant 5 of §6.1 in force: a
   `READY=1` from the process that crashed cannot mark its
   replacement ready. A submitted job has no generation and no
   replacement, so for it the check stops at step 4.

Anything that fails is dropped and recorded as a `peinit.notify.rejected`
event carrying the sender's PID (`subject.process.pid`), the sender's
service, job and activation when peinit could attribute it, and the
reason as one of a closed set of words in `outcome.reason` —
`unauthenticated-sender`, `generation-mismatch`, `malformed-line`, and so
on (§8.4). [*notify.a-rejected-datagram-is-dropped-and-recorded]

The UID and GID in the credentials are parsed and never used.
[*notify.the-credential-uid-and-gid-are-not-policy-inputs] They are
not policy inputs, and peinit does not consult them for anything —
identity on Peios is a token, and the token here is established by
which job the sender *is*, not by what UID it claims.

## Applying a datagram

A datagram can carry several newline-separated lines, and peinit applies
every one. [*notify.every-line-of-a-datagram-is-applied] Parsing happens
before application and is all-or-nothing: if
any line is malformed, nothing from that datagram is applied, and any
descriptors it carried are dropped and closed.
[*notify.a-malformed-line-voids-the-whole-datagram] Partial application
of an ambiguous service-control message is structurally impossible
rather than merely avoided.

A rejection is recorded after authentication, so the event can name the
service where one could be established.

## What the fields do

Most are handled elsewhere: `READY=1` and `RELOADING=1` in §6.5,
`WATCHDOG=1` and `WATCHDOG_USEC` and `EXTEND_TIMEOUT_USEC` in §6.6,
`STOPPING=1` in §12.2, and the fd store fields in §10.6. What each does
to a submitted job — and which are ignored for one, since a job has no
reload, watchdog or store — is in §8.5.

Six are event-emitting. `STATUS=`, `ERRNO=`, and `EXIT_STATUS=` are
authenticated and then emitted as KMES events —
`peinit.notify.status.reported`, `peinit.notify.errno.reported`,
`peinit.notify.exit-status.reported` — whose payloads carry the sender
as `subject.service.name`, `subject.job.guid`,
`subject.job.activation-generation` and `subject.operation.guid`,
alongside the value: `notify.status`; `notify.errno`, the number the
service sent, negated as every errno field is; `notify.exit-status`. A
value that is not a number is left out and the event still written.
[*notify.status-errno-and-exit-status-emit-events] They take the same
path as job and operation events, not a forward to eventd.
`peinit.notify.status.reported` is a `verbose` type, written only when
the emission policy turns it on (§8.4).

`STOPPING=1` emits `peinit.notify.stopping.reported`, carrying the same
attribution and no value. [*notify.stopping-emits-an-event] It is there
because the
field's only effect is the *absence*
of an action — peinit suppresses the SIGTERM (§12.2) — and an absence
cannot be inferred from what happened afterward. Without the event, a
service that was stopping and correctly received no SIGTERM looks
identical to one that should have received it and did not.

`PROGRESS=` and `PROGRESS_UNIT=` emit `peinit.notify.progress.reported`
(a `verbose` type), carrying the same attribution and the progress as
retained after the datagram — `notify.progress.current`, `.total`,
`.bounded` and `.unit`, the fields of a submitted job's
`peinit.job.status.reported`. A
datagram carrying either yields one event, not one per line, and at
most one a second for each activation: a datagram inside the second
updates what a status query reports and emits nothing, so a service
updating a thousand times a second produces one event a second.
[*notify.progress-emits-at-most-one-event-a-second]

`READY=1` and `RELOADING=1` emit nothing, deliberately: both are
observable through the state transitions they cause.
[*notify.ready-and-reloading-emit-no-event]

`STATUS=` is additionally stored on the service's runtime state and
exposed as `status_text` in a status query.
[*notify.status-is-exposed-as-status-text] It is cleared to null at the
start of every activation generation, in the same step that increments
the generation, so a status string cannot survive a restart and describe
a process that has been removed.
[*notify.status-text-is-cleared-on-each-activation-generation]

`PROGRESS=` and `PROGRESS_UNIT=` are stored the same way and exposed
together as `progress` in a status query, in the form a submitted job's
view gives it: `{current, total, bounded, unit}`, or null until the
service sends a `PROGRESS=`. [*notify.progress-is-exposed-as-progress]
Each is parsed as for a submitted job (§8.5): a value outside
`PROGRESS=`'s three forms, a `T` of zero, an `N` above `T`, or a unit
other than `bytes`, `items` or `percent` is dropped, never repaired, and
the rest of the datagram is applied. Each is replaced only by a datagram
that carries it. Both are cleared with `status_text`, in the same step.
[*notify.progress-is-cleared-on-each-activation-generation]

`ERRNO=` and `EXIT_STATUS=` are not stored. They are emitted and
otherwise not retained. [*notify.errno-and-exit-status-are-not-stored]

## Bounds [*notify.the-datagram-and-descriptor-bounds]

A datagram is read into a fixed 64 KiB buffer, and the control message
buffer is sized for 64 descriptors. Both `MSG_TRUNC` and `MSG_CTRUNC`
are inspected on the way in, and a datagram that overran either buffer
is refused as truncated rather than acted on in part: a larger datagram
is not applied from its surviving prefix, and a send carrying more than
sixty-four descriptors is not applied from the ones that fitted.
