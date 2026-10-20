---
title: The Control Socket
description: The Unix stream socket every runtime command arrives on — how it is created and protected, and the peer token it captures.
---

peinit serves every runtime command on a Unix stream socket at
`/run/services/peinit/control.sock`, created during Phase 1
infrastructure setup and existing for the lifetime of the system. Its
wire protocol is specified in PSPU §4; this chapter is how peinit
implements its side. A second socket beside it, the jobs socket, is the
door for submitting jobs rather than administering services; it is
§10.7.

## Creation and protection

The socket is created with `SOCK_CLOEXEC | SOCK_NONBLOCK` and a listen
backlog of 32, and unlinked when peinit drops it.
[*control.the-listener-flags-and-backlog] Accepted connections come from
`accept4` with both flags, so no connection descriptor is ever inherited
by a service. [*control.no-connection-descriptor-is-inherited]

peinit sets **no POSIX mode bits** on the socket, on the notification
socket, or on anything else it creates.
[*control.no-posix-mode-bits-are-set] Under KACS, mode bits are not
what governs access — a Security Descriptor is — so setting them would
be inert.

What governs access is the descriptor on the socket inode, which the
kernel checks at `connect()` before any peer identity is established.
peinit creates `/run/services/peinit/` and stamps the control socket
explicitly, after binding and before anything can connect
[*control.the-socket-descriptor]:

```text
O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;FW;;;AU)
```

Every authenticated principal can connect. `FW` is the file-write generic
right, which is what `connect()` on a pathname socket needs. Reaching the
socket grants nothing on its own: every command is checked against the
control descriptor or the target service's descriptor, with the token
captured at accept (§10.2). Admitting only SYSTEM and Administrators would
make that check moot for everyone else, since a descriptor that grants a
principal something could never be reached by them.

The notification socket is stamped explicitly too, with a descriptor of
its own that grants SYSTEM and the Service group rather than
Administrators (§10.5). The jobs socket carries a broader descriptor
again, because reaching it is a different permission (§10.7).

## Connections

peinit accepts a connection, obtains the peer's token, and only then
admits it against the connection limits:

| Key | Default | Meaning |
|---|---|---|
| `Machine\System\Init\MaxControlConnectionsPerUser` | 16 | Concurrent connections one caller can hold, counted by the user SID of its token. SYSTEM is exempt. [*control.max-control-connections-per-user] |
| `Machine\System\Init\MaxControlConnections` | 256 | Concurrent connections in all: a bound on peinit's descriptors, set well above what callers reach. [*control.max-control-connections] |
| `Machine\System\Init\MaxRequestSize` | 65536 | Maximum request size, in bytes. [*control.max-request-size] |
| `Machine\System\Init\ConnectionTimeout` | 30 | Seconds before an idle connection is closed. [*control.connection-timeout] |

The per-user limit is what keeps one caller from taking the connections
everyone shares: with every authenticated principal able to connect, a
single overall limit would let anyone fill it and lock administrators
out. SYSTEM is exempt, as it is from `MaxJobsPerSubmitter`, because what
it does is the machine's.

A connection over either limit is closed at the socket level, before any
request is read and without a response — there is no error code for it,
because there is no protocol state in which to deliver one. A peer whose
token cannot be obtained is closed the same way.
[*control.an-inadmissible-connection-is-closed-without-a-response]

The jobs socket has its own three, under `Machine\System\Init\`:
`MaxJobsConnections` (64), `MaxJobMessageSize` (32768) and
`JobsConnectionTimeout` (30), plus a fourth bound that is the
submitter's rather than the connection's, `MaxJobsPerSubmitter` (64).
All four are read at boot and on reload-config, and are listed with the
other operational keys in the registry key reference.

## The peer token [*control.the-peer-token-is-read-once-at-accept]

The token is read **once**, when the connection is accepted, through
the kernel's peer-token socket option (`getsockopt(SOL_KACS,
KACS_SO_PEER_TOKEN)`; the Peios Kernel TRM §3.5). It is the identity the
peer thread was acting under when it connected, so a peer that was
impersonating is captured as the impersonated identity
[*control.an-impersonating-peer-is-captured-as-the-impersonated-identity]
— which is what makes access decisions reflect the identity a client is
actually operating under rather than its underlying service identity.

Because it is captured once, a peer that changes identity mid-connection
is still evaluated against the identity it connected with.

## Idle and waiting

A connection is idle only when it has nothing in flight. One blocked on
a `wait=true` operation, on a `job-stop` with `wait`, or with output
still buffered, is never idle and is never closed by
`ConnectionTimeout` [*control.a-connection-with-work-in-flight-is-never-idle]
— it stays open until the operation or job
resolves, bounded by the operation's own timeout rather than the
connection's. A job wait has no timeout of its own; it is bounded by
the job.

A wait is answered when its operation reaches a terminal state, with
the lifecycle acknowledgment carrying the service's state at that
moment. Two cases are answered differently. An operation that failed
because peinit could not execute it (§8.2) is answered with an
`INTERNAL_ERROR` whose message carries the operation's result, not with
an `ok` carrying an unchanged state. And an operation whose service has
been discarded by the time it resolves — a `stop` that drained a
definition-removed entry (§10.3) — is answered with the state the
service last had, since there is no entry left to ask. Neither answer
costs the connection or the listener.

peinit handles one frame per readiness turn, and processes no further
frames from a connection while a wait is pending on it. Pipelined
requests are therefore serialized behind a wait.
<!-- lint-disable-next-line spelling -->
[*control.pipelined-requests-serialise-behind-a-wait]

## Timestamps [*control.timestamps-are-projected-from-the-monotonic-clock]

Every timestamp peinit puts on the wire is derived by projecting a
monotonic event stamp through the current offset between the realtime
and monotonic clocks. Elapsed-time decisions stay monotonic; only the
presentation is wall-clock.
