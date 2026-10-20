---
title: The Jobs Socket
description: The second socket — sequenced-packet, reachable by every authenticated principal — on which a job is submitted and managed, how peinit accepts and serves a connection, and how a wait is answered.
---

peinit serves job submission on a second Unix socket at
`/run/services/peinit/jobs.sock`, created in Phase 1 infrastructure
setup beside the control socket (§2.3) and existing for the lifetime of
the system. Its wire protocol is PSPU §7; this article is how peinit
implements the transport. What a submission *is* — identities, the
definition, the job's life — is §8.5.

## Why a second socket, and why sequenced packets

The control socket's descriptor admits the principals allowed to
change the system's services. Submitting a job is a different
permission held by a different population, and putting the two on one
socket would force one descriptor to serve both. Two sockets let the
filesystem say who can submit and who can administer, separately,
with no policy inside peinit.

The socket is `SOCK_SEQPACKET` rather than a stream
[*jobs.the-socket-is-sequenced-packet] because a message
on it carries more than bytes: the token the job is to run as, and the
descriptors it is to be given, as ancillary data. The kernel ties
ancillary data to the record it was sent with, and a sequenced-packet
socket makes one `send` one message and one identity, so peinit never
has to decide which request an attached token or descriptor belongs
to. The cost is that a message has a maximum size and cannot be split;
a definition is small.

## Creation and protection

The listener is created with `SOCK_CLOEXEC | SOCK_NONBLOCK` and a
backlog of 32; a stale path is unlinked before the bind, and
connections are accepted with `accept4` under the same flags.
[*jobs.the-listener-flags-and-backlog] After
binding, and before anything can connect, peinit stamps the socket
inode [*jobs.the-socket-descriptor]:

```text
O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;FW;;;AU)
```

`FW` is the file-write generic right, which is what a Unix `connect()`
on a pathname socket needs, so every authenticated principal can
connect and SYSTEM and Administrators can additionally change the
descriptor. **Being able to connect is the permission to submit.** The
kernel checks this descriptor at `connect()`, and peinit performs no
access check of its own before a `submit`
[*jobs.connecting-is-the-permission-to-submit]; an administrator who
wants a narrower or wider population changes the descriptor on the
socket, not anything in peinit. What a submitter can then do to a job is
decided by the job's own descriptor (§8.5), never by this one.

A failure to bind or stamp the socket sends peinit to recovery, as the
control socket's does: a system whose jobs door is missing is
administrable, but one whose door exists with the wrong descriptor is
either unreachable or open to everything, and neither is a state to
boot into.

## Connections

The listener is one event source of the runtime loop; each accepted
connection is another, keyed by its descriptor. On accept peinit
captures the peer's identity once — the peer token, as the control
socket does (§10.1), and the peer's process handle through
`SO_PEERPIDFD` — and only then admits the connection against the
limit [*jobs.the-peer-token-and-pidfd-are-captured-on-accept]:

| Key | Default | Meaning |
|---|---|---|
| `Machine\System\Init\MaxJobsConnections` | 64 | Concurrent connections. [*jobs.max-jobs-connections] |
| `Machine\System\Init\MaxJobMessageSize` | 32768 | Maximum message content, in bytes. Half of KMES `MaxEventSize`'s default, so a job whose record fills a message still has its `peinit.job.ended` fit the ring (§8.4). [*jobs.max-job-message-size] |
| `Machine\System\Init\JobsConnectionTimeout` | 30 | Seconds before an idle connection is closed. [*jobs.jobs-connection-timeout] |
| `Machine\System\Init\MaxJobsPerSubmitter` | 64 | Live jobs one submitter SID can hold; SYSTEM exempt. See §8.5. |

A connection over the limit, or one whose peer cannot be identified,
is closed at the socket level without a response.
[*jobs.an-inadmissible-connection-is-closed-without-a-response] The
pidfd is held for the life of the connection, because a `submit` with
no token attached opens the job identity through it (§8.5).

A connection carries an identity and nothing else. Closing it does not
affect any job submitted on it: a job belongs to its submitter's
identity, and a submitter that reconnects finds its jobs where it left
them. [*jobs.closing-a-connection-does-not-affect-its-jobs]

## Messages

Every message is received with room for one attached token and for 64
descriptors, the output sink included.
[*jobs.a-message-carries-one-token-and-up-to-sixty-four-descriptors] One
token is all a message can
carry: the kernel refuses a second `KACS_SCM_TOKEN` control message at
send time, so the "more than one attached token" refusal in PSPU §7.5
is enforced before peinit ever sees the record. Two truncations are
told apart.
Content the kernel truncated (`MSG_TRUNC`) is `REQUEST_TOO_LARGE`, and
that closes the connection, since the transport has lost a record.
[*jobs.a-truncated-message-is-request-too-large]
Ancillary data the kernel could not fit (`MSG_CTRUNC`) is
`INVALID_ARGUMENTS`, with the connection kept: the content is intact,
but a request whose attachments were partly lost does not describe the
job the submitter meant, and peinit does not act on it.
[*jobs.truncated-ancillary-data-is-invalid-arguments] Every
descriptor a message carried that is not handed to a job or adopted as
a sink is closed, on every path.
[*jobs.every-unused-descriptor-is-closed]

A response is one compact JSON object, no terminator.
[*jobs.a-response-is-one-compact-json-object] A `submit`
answered with a running job carries a duplicate of the job's pidfd as
`SCM_RIGHTS` on the response record; nothing else carries ancillary
data. [*jobs.a-submit-answer-carries-the-jobs-pidfd]

Only `REQUEST_TOO_LARGE` closes the connection after an error. Every
other error is answered and the connection kept.
[*jobs.only-request-too-large-closes-the-connection]

## The turn

peinit handles one message per readiness turn on a connection, and
reads nothing from a connection while a wait is pending on it, so
pipelined messages serialize behind a wait.
<!-- lint-disable-next-line words spelling -->
[*jobs.pipelined-messages-serialise-behind-a-wait] A turn either answers
at once, or records a pending wait on the connection state, or does both
in the case of a refusal. Three waits exist:

| Wait | Set by | Answered when |
|---|---|---|
| Submit | `submit` | The job leaves `created`: exec confirmed, or the launch failed. [*jobs.the-submit-wait] |
| Wait | `wait` | The condition holds — terminal, or ready-or-terminal. [*jobs.the-wait-wait] |
| Stop | `stop` with `wait` | The job is terminal. [*jobs.the-stop-wait] |

Waits are flushed after the supervisor's work has been committed each
turn, and again whenever a job's state moves — a launch, a reap, a
canceled stop. The flush answers every satisfied wait with the job
view at that moment, and for a Submit whose job is running, the pidfd.
[*jobs.waits-are-flushed-when-a-jobs-state-moves]
An answer that cannot be built — a job whose record was purged before
the flush, or a pidfd that cannot be duplicated — is that connection's
error record, `UNKNOWN_JOB`, or `INTERNAL_ERROR`; it never aborts the
flush of every other wait.
[*jobs.an-unbuildable-answer-does-not-abort-the-flush]

A connection with a pending wait, or with a response still queued, is
not idle and is never closed by `JobsConnectionTimeout`.
[*jobs.a-connection-with-a-pending-wait-is-never-idle] A wait has no
timeout of its own; it is bounded by the job, and a submitter that
needs a bounded wait polls `status` or waits on the pidfd it was
given.

## Commands

`submit`, `status`, `wait`, `stop` and `signal`, exactly as PSPU §7.8
defines them and §8.5 implements them. Every one but `submit` names a
job and is checked against that job's descriptor with the connection's
token; a denial is answered `ACCESS_DENIED`, and recorded by KACS as
`kacs.audit.access.checked` naming the job when the job's descriptor
audits it, as a default descriptor does (§8.4).
[*jobs.every-command-but-submit-is-checked-against-the-jobs-descriptor]

## Idle and shutdown

Idle connections are closed before and after each wait of the event
loop, and the next idle deadline is folded into the loop's wait
timeout. [*jobs.an-idle-connection-is-closed] During shutdown the socket
stays open: `submit` is refused
with `INVALID_STATE`, and the other four commands keep answering
(§12.2). [*jobs.submit-is-refused-during-shutdown]

## Timestamps

As on the control socket (§10.1): monotonic stamps projected through
the current realtime offset at the moment of answering.
