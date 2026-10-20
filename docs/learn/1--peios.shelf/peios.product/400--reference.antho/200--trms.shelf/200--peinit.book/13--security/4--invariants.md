---
title: Security Invariants
description: The properties peinit does not violate, and the code paths that make each of them structural.
---

Properties peinit does not violate.

**1. peinit does not grant privileges it was not asked to grant.**
`RequiredPrivileges` is subtractive. peinit removes privileges and never
adds one — there is no code path that constructs anything but a removal
(§4.5).

**2. peinit does not bypass AccessCheck for control operations.** Every
control command reaches a check against the appropriate descriptor.
There is no backdoor, no override flag, and no "trust localhost". Every
command on a submitted job, on either socket, is checked against the
job's own descriptor — the submitter included, and the job identity
excluded unless the descriptor says otherwise (§8.5).

**3. peinit does not expose one service's state to another without
access control.** `list` returns only what the caller can query, and
`status` is checked per service; `job-list` and `job-status` are
checked per job (§10.2).

**4. No access denial is silent by default.** Every check names its
object to KACS in an audit context, and every descriptor peinit builds
itself — the default service descriptor, the control descriptor, a
submitted job's default — carries a SACL auditing every refusal, so a
failed AccessCheck is recorded by KACS as `kacs.audit.access.checked`
with the caller, the object and the access requested and granted
(§8.4). A descriptor written to the registry or supplied with a job is
honored as it is, SACL included; whoever can write one is trusted
with that object's audit policy (§4.6). The `MAXIMUM_ALLOWED` check
that fills a response's `granted` refuses nothing (§10.2).

**5. The control descriptor, the ServiceSecurity descriptors, and the
per-job descriptors are the only policy inputs for runtime access
control.** [*invariant.the-descriptors-are-the-only-policy-inputs]
peinit consults no configuration file, no environment variable, and no
hardcoded principal list. The only inputs to AccessCheck are those
descriptors — the first two sourced from the registry with a compiled-in
default, the third fixed at submission from the submitter's SID or the
SDDL it supplied. Who can *submit* is not a policy input at all: it is
the jobs socket's file descriptor, decided by the filesystem before
peinit sees the connection.

**6. peinit does not share its SYSTEM token.** It opens its own token
query-only, as a template, and never installs it on a child. Even an
`Identity=SYSTEM` service gets a separately minted token.

**7. peinit does not drop its SYSTEM identity.** PID 1 runs as SYSTEM
for the lifetime of the system. Only the forked child installs a token;
peinit never installs one on itself.
[*invariant.peinit-never-installs-a-token-on-itself]

**8. Identity is deterministic, and the dangerous case is never
implicit.** Every service runs with a known identity. `SYSTEM` has to be
declared explicitly; an absent or empty `Identity` means `LocalService`.

The identity parser maps an absent or empty value to `LocalService`.
It selects `SYSTEM` only when the definition names it. After authd mints a
token, peinit checks that its user SID matches the declared identity. A
mismatch fails the launch (§4.3).

**9. A submitted job runs only as an identity the kernel verified its
submitter could convey.** There is no identity field, no route from a
plain descriptor to a job identity, and no token opened through a
PID or descriptor the submitter named. The job identity is the
connecting process's own primary — reached through the kernel's handle
on that process — or the token the kernel attached to the message after
running the impersonation gates with the submitter as installer.
peinit never re-derives that decision and never refuses a token for
who it names (§8.5).
