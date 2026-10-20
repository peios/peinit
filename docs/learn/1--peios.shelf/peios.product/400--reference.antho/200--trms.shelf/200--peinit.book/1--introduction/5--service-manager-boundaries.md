---
title: Service Manager Boundaries
description: The service, job and operation model, compatibility boundaries, event-loop invariant and dependencies of PID 1.
---

These design notes explain the boundaries behind the [operator
workflows](~peios/services-and-jobs/overview). The implementation chapters
specify each mechanism in detail.

## peinit in one sentence

**peinit is the sole service manager and PID 1 — it reads service definitions
from the registry, starts them in dependency order under per-service
identities, supervises them through a defined state machine, and tears them
down in reverse at shutdown.**

Everything in this topic is an elaboration of that sentence: what a *service
definition* is, what *dependency order* means, what the *state machine* looks
like, how *identity* is materialised, and how you drive the whole thing from a
shell.

## Three kinds of object

peinit is easiest to understand through the three first-class objects it works
with. Keeping them straight is most of the battle when you read a status
output or an event log.

| Object | What it is | Lifetime |
|---|---|---|
| **Service** | A *definition* — a named unit of execution with identity, policy, and configuration, stored in the registry. It has a runtime state (the [state machine](~peios/services-and-jobs/the-service-lifecycle)) and, when running, a process. | Long-lived. Persists across restarts and reboots. |
| **Job** | A single *process execution*. Every time peinit forks — a service's main process, a hook, a health check, a submitted job — that is one job, with its own GUID and exit result. | Short-lived. A restart creates a *new* job. |
| **Operation** | A *requested action* on a service — start, stop, restart, reload, reset. Control commands create operations that are validated, queued, and executed. | Short-lived. Resolves to a terminal state, then is dropped. |

A service is the *what*; a job is *what actually ran*; an operation is *what
someone asked for*. A single `restart` command, for example, is one
**operation**, which stops the current **job** and starts a new one, all
against one **service**. Jobs and operations both carry GUIDs and are emitted
to [eventd](~kernel/internals/kacs/auditing/overview) as they complete, which is how the
history of a service is reconstructed after the fact. They get their own page:
[Jobs and operations](~peios/services-and-jobs/jobs-and-operations).

Underneath all three sits the fourth thing peinit is always handling — the
**token**, the identity a service runs under. Tokens are not peinit's
invention; they are the [system-wide identity object](~kernel/internals/kacs/tokens/the-token-model).
peinit's role is to obtain the right token for each service and install it
before the process runs. That is the subject of [Service identity and
privileges](~peios/services-and-jobs/identity-and-privileges).

## What peinit is not

peinit borrows vocabulary from init systems you can already know, and the
familiarity is a trap. Clearing three wrong mental models is most of
understanding what peinit actually is.

**It is not systemd.** There are no unit files. peinit does not read, parse,
or translate `.service` files, and there is no migration shim. A service
definition is a key in the [registry](~peios/sdk-registry/overview) under
`Machine\System\Services\`, made of typed registry values — not a text file
under `/etc`. Some *interfaces* are deliberately compatible where that helps:
peinit speaks the `sd_notify` readiness protocol, and timer schedules use
systemd's `OnCalendar` calendar-expression format. The compatibility stops at
those wire formats; the model underneath is different.

**It is not Windows SCM.** The influence is real — services as securable
objects with per-service descriptors, token-based service identity, an
access-controlled control interface, a structured state machine — but it is
*architectural*, not interface-level. peinit does not implement the SCM RPC
protocol, Windows service types, or Windows control codes.

**It is not a logging system.** peinit holds a service's `stdout` /`stderr`
pipes at birth, so it controls *where* output goes — but storage, indexing,
and queries are [eventd](~kernel/internals/kacs/auditing/overview)'s job. peinit forwards;
eventd is the historian. The same split applies to job and operation history:
peinit emits structured events and forgets them; eventd keeps them. See
[Service output and logging](~peios/services-and-jobs/output-and-logging).

A fourth, smaller correction: **peinit does not supervise forking daemons.** A
service that double-forks to "daemonise" is solving a problem that does not
exist when the manager tracks the process it spawned. peinit tracks every
process by [pidfd](~peios/services-and-jobs/service-types); a legacy binary
that insists on double-forking must be wrapped at the package layer.

## The one operational invariant

peinit is **single-threaded PID 1**, and that one fact explains a surprising
amount of its design. In a single-threaded PID 1, a blocking system call
blocks *everything* — child reaping, watchdog expiry, shutdown signals, every
other event stops while the call is stuck. So peinit's hard rule is that **its
event loop never blocks on a userspace service.**

Two consequences you will meet repeatedly:

- **peinit operates on an in-memory snapshot of the registry, not a live
  view.** It reads service definitions at boot and on configuration reload,
  and thereafter works from an in-memory model that it updates from change
  notifications. Supervision never waits on
  the registry being available. This is why a configuration change does not
  always take effect immediately — see [Defining a
  service](~peios/services-and-jobs/defining-a-service).
- **Anything that could block is pushed off the main loop.** Filesystem
  condition checks run in a short-lived forked helper rather than a `stat()`
  on the event loop; token requests to authd are driven by a timed state
  machine, not a blocking call; log delivery is best-effort and never exerts
  back-pressure on peinit.

You do not need to think about the event loop to operate peinit, but it is the
reason behind several behaviors that would otherwise look arbitrary.

## Where peinit sits in the system

peinit is part of the [Trusted Computing
Base](~kernel/internals/kacs/start-here/boot-and-trust-invariants): a compromise of peinit is
a compromise of the whole machine. It runs as
SYSTEM with all privileges, and it
never drops that identity. It depends on, and is depended on by, the other TCB
components:

| peinit relies on | For |
|---|---|
| **KACS** | Service identity (installing tokens on children), control-interface authentication (peer token + AccessCheck), and per-service access control. See [Access decisions](~kernel/internals/kacs/access-check/the-dacl-walk). |
| **LCS / the registry** | Service definitions, boot configuration, and its own parameters. See [The registry](~peios/sdk-registry/overview). |
| **registryd** | The registry *source* daemon — the first service peinit starts. peinit treats it as an opaque dependency. (`registryd` is an interface; the default implementation is `loregd` — see [Boot and boot modes](~peios/boot/boot-and-boot-modes).) |
| **authd** | Minting tokens for non-platform services. peinit requests; authd mints. |
| **eventd** | Service logs (forwarded over a socket) and job/operation/audit events (emitted through KMES). |
| **KACS, again** | Conveying a job identity: when a process submits a job over peinit's jobs socket and attaches a token, the kernel gates the attach — so peinit can run a program as a service's client without ever judging the identity itself. See [Jobs and operations](~peios/services-and-jobs/jobs-and-operations). |

The bootstrapping puzzle — authd needs the registry, the registry needs to be
started by *something*, and that something is peinit running before any of
them exist — is resolved by the boot model in [Boot and boot
modes](~peios/boot/boot-and-boot-modes).

## How a token is materialised

The `Identity` field decides where the token comes from:

| `Identity` value | Token source |
|---|---|
| `SYSTEM` | **Minted by peinit** from its own SYSTEM identity. |
| Any other principal name or SID | **Requested from [authd](~peios/peinit/service-identity/the-authd-path).** |
| Absent or empty | authd, defaulting to `LocalService`. |

### The SYSTEM path

For a `SYSTEM` service, peinit mints an independent SYSTEM token using its own
token as the template: it reads its own identity (user SID `S-1-5-18`, the
group list, the privilege set) and mints a fresh primary token with the same
identity, adding the service's [per-service SID](#the-per-service-sid). The
minted token is fully independent — the [privilege
trimming](~peios/services-and-jobs/identity-and-privileges#privilege-restriction)
that follows affects only this token, never peinit's.

This path exists because of a bootstrapping problem: the platform services
that *make* the normal token flow possible cannot use it, because they have to
start *before* it exists. `registryd` and `authd` use this bootstrap path
during [boot](~peios/boot/boot-and-boot-modes). Standard eventd
and lpsd deployments now use `Identity=Service`, take the authd path, and
start after authd (§4.2).

> [!NOTE]
> There is no allow-list restricting which services *can* be `SYSTEM`. The
> security boundary is the [registry key
> descriptor](~peios/services-and-jobs/who-can-manage-a-service) on
> `Machine\System\Services\` — an administrator who can create service
> definitions is already trusted to assign any identity, so a second gate
> would add nothing. The dangerous case is never *implicit*, though: `SYSTEM`
> must be written out explicitly; an empty `Identity` defaults to the minimal
> `LocalService`, never to SYSTEM.

### The authd path

For any other identity, peinit asks
[authd](~peios/peinit/service-identity/the-authd-path) for a token: it
sends the `Identity` value verbatim, and authd routes it to the right source —

- **Well-known principals** (`LocalService`, `NetworkService`, …) → a
  built-in identity with a predefined minimal privilege set;
- **Local service accounts** → lpsd, the local
  principal store;
- **Domain accounts** → the directory connector;

— resolves the principal's SIDs, mints a token, creates a logon session, and
hands the token back to peinit to install. peinit neither knows nor cares
whether the identity is local or domain; routing is authd's job.

authd records each request as an `authd.service.attested` event: the service
(`object.service.name`), the identity it got (`object.token.sid`), its session
and the privileges policy granted it. A refused request is recorded too, with
the reason, such as `account-restricted` for an identity that can not be used
for a service logon. The event is standard, so the emission policy can switch
it off on a machine that restarts services often.

> [!IMPORTANT]
> Every non-SYSTEM service start depends on authd. peinit interacts with it
> over a non-blocking, timed channel — if authd is unreachable or
> unresponsive, the service start *fails* rather than hanging PID 1. A broken
> authd therefore prevents non-SYSTEM service starts, including any platform
> service configured that way, while SYSTEM-minted services do not need this
> request. The authd interface is `ServiceAttest` on `/run/logon.sock`,
> specified in PGSS §2.19 and used through libauthd (§4.3).

## The per-service SID

Every service token — minted or authd-issued — carries a **per-service SID**
in its group list. It uses authority `S-1-5-80` and is derived
deterministically from the service name: the SHA-1 of the uppercased name (as
UTF-16LE), with the digest split into five sub-authorities. authd adds it
automatically for the tokens it mints; peinit computes and adds it itself for
SYSTEM tokens, no authd involvement needed.

The point of the per-service SID is **fine-grained access control even when
services share an identity.** A dozen services might all run as `LocalService`
, but each has a *unique* service SID, so an ACL can grant access to exactly
one of them without inventing a dedicated account. It also keeps services
configured as SYSTEM distinguishable to
[AccessCheck](~kernel/internals/kacs/access-check/the-dacl-walk). See Well-known
principals for the SID landscape this
fits into.

## A clean context

A service inherits **only** what peinit explicitly hands it — its standard
streams and any [stored file
descriptors](~peios/services-and-jobs/execution-environment#the-fd-store).
Every other descriptor peinit holds — the control socket, the notify socket,
the event-loop fd, the registry, and authd and eventd connections — is opened
close-on-exec, so it closes automatically at exec and can never leak into a
service. peinit also resets the child's signal state to defaults (it runs with
all signals blocked for its own [signalfd](~peios/boot/shutdown);
a service must not inherit that). The guarantee is that a service starts from
a clean slate, not from peinit's privileged one.

## The cgroup tree

Every service runs in its own
cgroup tree under
`/sys/fs/cgroup/peinit/`, with separate sub-cgroups for the main process,
hooks, and health checks:

```text
/sys/fs/cgroup/peinit/<service>/
                       ├── main/     the main process
                       ├── hooks/    pre/post hooks
                       └── health/   health checks
```

peinit uses cgroups for two things only: **tracking** every process a service
spawns (so a child that forks its own children is still accounted to the
service), and **clean kill** (terminating the entire tree at once, so nothing
is orphaned). It does *not* use cgroups for resource accounting or limits.

The split into sub-cgroups serves a real purpose: it satisfies cgroup v2's "no
internal processes" rule, and it lets peinit kill a service's *hooks* or
*health checks* without touching the main process.

When an old tree cannot be removed — because a process survived SIGKILL in
[D-state](~peios/services-and-jobs/the-service-lifecycle) and leaked it —
peinit creates a fresh **generational** tree (`<service>.gen<N>`) for the next
start, so a stuck old instance never blocks a new one. Leaks are surfaced in
the service's `warnings`; they are a sign of an underlying I/O fault, covered
in [Keeping services running](~peios/services-and-jobs/supervision).
