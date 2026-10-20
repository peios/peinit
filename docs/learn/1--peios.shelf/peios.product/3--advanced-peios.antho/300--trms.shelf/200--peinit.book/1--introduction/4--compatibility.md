---
title: Compatibility and Prior Art
description: peinit is not a port of anything — where it meets sd_notify, the fd store, calendar expressions and the Windows SCM, and what it deliberately omits.
---

peinit is not a port or a reimplementation of anything. The choices that
give it its shape — identity as a token, configuration in the registry,
operations as objects, jobs as observable executions — were made for
Peios. But several interfaces deliberately match existing conventions,
because the convention is good and breaking it buys nothing.

## sd_notify

peinit speaks systemd's sd_notify datagram protocol: a service reports
readiness, keepalives, status, and stored descriptors by sending
`KEY=VALUE` lines to the socket named in `NOTIFY_SOCKET`. Existing
software that supports sd_notify works unmodified.

The compatibility is not total. `MAINPID=` is not supported,
[*compat.mainpid-is-not-supported] because peinit does not supervise
forking daemons — it tracks the process it forked through a pidfd, and
there is no way to redirect supervision somewhere else. `BUSERROR=` is
not supported [*compat.buserror-is-not-supported] because Peios has no
D-Bus.

The protocol as peinit speaks it, including which fields it accepts and
how a sender is authenticated, is specified in PSPU §4.

## The fd store

`FDSTORE=1`, `FDNAME=`, `FDSTOREREMOVE=1` and `FDPOLL=0` work as they do
under systemd, and descriptors come back to a restarted service at fd 3
onwards with `LISTEN_FDS` and `LISTEN_FDNAMES` set. A daemon written to
survive a restart without dropping its listening sockets keeps working.

## Calendar expressions

Timer schedules use systemd's `OnCalendar` format, including weekday
names, lists, ranges, repetition, the `~` last-day-of-month form, IANA
timezone suffixes and the named shortcuts. §9.1 gives the grammar peinit
actually parses; the one deliberate subtraction is sub-second precision,
[*compat.sub-second-precision-is-not-parsed] which service scheduling
has no use for.

## Service

The service model owes its architecture to the Windows SCM: services as
securable objects carrying their own descriptors, identity as a token
rather than a user account, an access-controlled control interface, and
a structured state machine. The debt is architectural. peinit implements
none of the SCM's RPC protocol, none of its service types, and none of
its control codes.

## What is deliberately absent

There is no systemd unit file support: no reader, no parser, no
generator, no migration path. Service definitions are registry keys.
Roles are how a package declares them.

There is no socket activation in the systemd sense — peinit does not
listen on a service's behalf and hand it a connection. The fd store
covers the case that matters, which is a service keeping its own
listener across its own restart.

There is no resource control. peinit uses cgroups for tracking and for
clean kill, and sets `RLIMIT_NOFILE` and `RLIMIT_CORE` if a definition
asks, [*compat.rlimit-nofile-and-core-are-the-only-limits] but it does
not do accounting, slices, or limits.

## Features that belong to other components

| Concern | Component |
|---|---|
| Authentication and token minting | authd |
| The local identity database | lpsd |
| Log storage, indexing,  and queries | eventd |
| Registry storage | registryd, over LCS |
| Packaging and installation | peipkg and the role system |
| Device management | eudev |
| Network configuration | a dedicated service |
| File access control enforcement | FACS |
