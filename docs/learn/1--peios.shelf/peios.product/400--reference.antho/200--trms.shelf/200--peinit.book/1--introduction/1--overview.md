---
title: Overview
description: peinit is PID 1 and the only service manager on a Peios system — what makes it not systemd, the shape of the daemon, and what it is not.
---

peinit is PID 1. [*intro.peinit-is-pid-1] It is the only service manager
on a Peios system: every supervised process on the machine — a platform
daemon, an application service, a startup hook, a health probe, a job
some other service asked for on a user's behalf — is forked by peinit
and watched by peinit until it exits.
[*intro.every-supervised-process-is-forked-by-peinit]

It is a single-threaded Rust process.
[*intro.peinit-is-single-threaded] That is the constraint the rest of
its design answers to. A blocking syscall in PID 1 stops everything:
child reaping, watchdog expiry, shutdown signals, the control socket. So
peinit keeps a complete in-memory model of every service it knows about,
reads the registry synchronously only at boot and when it reloads —
whether an administrator asked for the reload or a registry change
notification set it off, which takes the same path (§10.4)
[*intro.the-registry-is-read-at-boot-and-on-reload] — and pushes
anything that could block off the main loop into a forked helper or a
pollable descriptor.

## What makes it not systemd

Two things, and both run deep enough to change the shape of the daemon.

**Services are securable objects.** A service carries a Security
Descriptor that says who can start it, stop it, query it, or reload it,
and peinit evaluates that descriptor against the caller's KACS token on
every control request. There is no "root can do anything" path, because
there is no root — there is a token, and AccessCheck is the only thing
that decides.

**Service identity is a token, not a user.** peinit never sets a UID, a
GID, or a Linux capability on a service process.
[*intro.no-uid-gid-or-capability-is-set] It obtains a KACS token
— minted from its own for the platform daemons that start before an
authority exists, requested from authd for everything else — restricts
its privileges to what the definition asked for, and installs it on the
child before exec. Every service also carries a per-service SID derived
from its name, so two services sharing an identity are still
distinguishable to an access check.

Configuration follows from the second one: service definitions live in
the registry, under `Machine\System\Services\`, where they are protected
by the registry's own descriptors rather than by file permissions. There
are no unit files, no generators, and no translation layer.

## The shape of the daemon

Boot is two phases with registryd as the boundary. Phase 1 is compiled
in and has no registry dependency at all: it confirms the root is
writable, mounts what is missing, restores entropy, settles the clock,
and starts registryd. Phase 2 reads the service graph out of the
registry and boots the system from it. When Phase 1 cannot complete,
there is no Phase 2 to fall back to, and peinit drops into a recovery
shell.

Once booted, peinit is an event loop over a handful of descriptors: a
signalfd, the control socket, the jobs socket, the notify socket, one
timerfd per armed timer, a pidfd per supervised process, a pipe pair
per supervised process's output, and the registry's change-notification
descriptor. Work arriving on most of them turns into an **operation** —
a queued, observable request to move a service through its state
machine — and executing an operation eventually forks a **job**. Work
arriving on the jobs socket skips the operation: a submitted job has
no service and no state machine to contend for, so a submission forks
a job directly.

Those two objects are how peinit stays comprehensible under concurrency.
Operations exist so that two administrators issuing conflicting commands
get a defined answer instead of a race. Jobs exist so that "what
actually ran" is a thing with an identifier, a token summary, an exit
status and a log correlation key, rather than a PID that can already
have been reused.

peinit keeps no history of either. A job or an operation that reaches a
terminal state is emitted as a structured event into the KMES kernel
ring buffer and dropped.
[*intro.a-terminal-job-or-operation-is-dropped] eventd is the historian.

## What peinit is not

It does not assemble storage. The initramfs delivers a mounted,
writable root, and peinit neither decrypts, nor assembles, nor checks
it. It has no mount feature beyond the fixed Phase 1 set
[*intro.no-mounting-beyond-phase-1] — mounting a data partition is a
Oneshot service's job.

It does not store logs. It holds the pipes at birth, tags each line, and
forwards it to eventd; before eventd exists it buffers, and when the
buffer fills it drops the oldest.

It does not authenticate anyone. authd mints tokens; peinit installs
them. It does not resolve identities, and it does not know or care
whether a principal is local or from a domain.

And it does not support forking daemons.
[*intro.forking-daemons-are-not-supported] It tracks the process it
spawned, through a pidfd obtained at fork, and there is no mechanism for
a service to point supervision somewhere else.
