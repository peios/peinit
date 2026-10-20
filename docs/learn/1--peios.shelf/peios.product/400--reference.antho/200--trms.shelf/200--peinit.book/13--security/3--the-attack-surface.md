---
title: The Attack Surface
description: What the socket descriptors mean in practice, and what the autorun directory exposes.
---

| Surface | Reachable by | Controls | Protected by |
|---|---|---|---|
| Control socket | Every authenticated principal, by default | Service lifecycle, shutdown, observing,  and stopping jobs | The peer token and AccessCheck against the target's descriptor for every command; a per-user connection limit |
| Jobs socket | Every authenticated principal, by default | Running a program under supervision as an identity the submitter holds; managing the jobs it submitted | The socket inode's descriptor, which is the whole of the submission permission; the kernel's gating of the attached token; the job's own descriptor for every later command; a per-submitter quota |
| Notification socket | Anything that can connect | Service readiness, watchdog, stored descriptors | The socket inode's descriptor, then PID matching verified through a pidfd, plus the start generation |
| Registry keys | Anything with registry access | Definitions, triggers, configuration | Registry key descriptors, enforced by LCS |
| `Machine\System\Init\EnvVars\` | Anything with registry access | The environment of every service | That key's descriptor, and nothing else — variable names are not filtered |
| Service cgroups | peinit | Process tracking and clean kill | Ownership of the hierarchy |
| Phase 1 mounts | peinit | Virtual filesystem availability | Hardcoded, no external input |
| `/lcl/policy/autorun.d` | Whatever can write it | Arbitrary code as SYSTEM in early boot | The directory's descriptor |
| Boot attempt counter | Whatever can write `/.peinit/` | Recovery mode entry | That file's descriptor |

## What the socket descriptors mean in practice

The control socket admits every authenticated principal (§10.1), and
what each can then do is decided per command by the control descriptor
and each service's own. The `ACCESS_DENIED` path and the KACS audit
record it produces are therefore reachable by anyone who can sign in: a
caller can make as many `kacs.audit.access.checked` records as it has
refused commands to send, because the default SACLs ask for one per
refusal; a descriptor whose SACL audits less makes fewer. So is peinit's
request
parser, which bounds every request by `MaxRequestSize` before parsing
it. What one caller can hold open is bounded by
`MaxControlConnectionsPerUser`, so no one caller can fill the
connections everyone shares.

The notification socket is stamped for SYSTEM and for the Service group
`S-1-5-6` (§10.5) — every process started as a service, and nothing
else. Administrators are deliberately absent from it: an administrator
has no business asserting that a service is ready, and the two sockets
have different populations however alike their paths look.

A connection to the notification socket from any other principal is
refused at `connect()`, by the filesystem, before peinit ever obtains a
peer token — which means no audit event records it.
[*surface.a-refused-connect-never-reaches-peinit]

The jobs socket is deliberately wider: `FILE_WRITE_DATA` for
Authenticated Users, full control for SYSTEM and Administrators
(§10.7). Connecting is the submission permission, and peinit adds no
check of its own behind it. What bounds the population that can reach
it is that descriptor, and what bounds what any one submitter can do
is the quota and the rule that a job runs only as an identity the
kernel verified the submitter held. The `ACCESS_DENIED` path *is*
reachable here — it is how a submitter learns it can not touch another
submitter's job — and every such denial is the job's SACL's to record:
under a job's default descriptor, KACS records each as
`kacs.audit.access.checked` naming the job (§8.4).

These moved together with §4.3, and had to. A service resolved to a
non-SYSTEM identity could not have reached the notification socket to
report that it had started, so making identity real while the socket
stayed SYSTEM-only would have broken every non-SYSTEM service's
readiness protocol at once. What makes the pair work is that the tokens
peinit mints for itself now carry `S-1-5-6` as well, so a `SYSTEM`
service and a `LocalService` one are on the same footing here.

## The autorun directory

The Phase 1 autorun step (§2.3) executes every file in
`/lcl/policy/autorun.d` as SYSTEM, before path provisioning and before
any service. It is fail-open by design, so nothing about a script going
wrong stops the boot.

Its only protection is the descriptor on that directory. Anything that
can write there executes as SYSTEM at the earliest point in userspace
that exists.
