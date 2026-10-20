---
title: Peinit's Privileges
description: peinit runs as SYSTEM with every privilege, and the much narrower set it actually exercises.
---

peinit runs as SYSTEM with every privilege for the lifetime of the
system. What it actually exercises is narrower.

| Privilege or capability | Used for |
|---|---|
| `SeCreateTokenPrivilege` | Minting SYSTEM tokens for platform services during bootstrap, before authd exists (§4.2). |
| `SeTcbPrivilege` | Requesting tokens from authd on a service's behalf, and installing a primary token on a child whose identity differs from peinit's own — a service's, or a submitted job's. |
| `SeAuditPrivilege` | Writing events: KMES refuses an emit from a token without it enabled. And recording access decisions: an AccessCheck peinit runs writes the `kacs.audit.access.checked` record its descriptor's SACL asks for only while the caller holds it enabled (§8.4). |
| Token duplication | Turning a job identity into the primary token a submitted job runs as (§8.5): opening the submitter's own primary through its pidfd, or taking the token the kernel attached, and duplicating either to a primary. SYSTEM holds full access on every token's default descriptor, which is what makes the duplicate permitted. |
| Process creation | Fork and exec, inherent to PID 1. |
| cgroup management | Creating and destroying trees under `/sys/fs/cgroup/peinit/`. |
| Signal delivery | SIGTERM and SIGKILL to managed processes. |
| Mount operations | The Phase 1 virtual filesystems. |

peinit verifies the three privileges at startup, before Phase 1 does
anything that needs them, and fails to recovery naming which is missing
[*privilege.the-required-privileges-are-verified-at-startup]:

```text
peinit: required privileges are not held: Token("peinit is missing
required privilege(s): SeCreateTokenPrivilege")
```

Present *and* enabled — a privilege the token carries but has not
enabled is not usable, and would fail identically to being absent.
`SeAuditPrivilege` is the exception: held but disabled, peinit enables
it before checking, rather than run with no events and no record of who
was refused what. The boot SYSTEM token PID 1 runs as holds every
privilege enabled, so on a token the kernel made the check passes and
changes nothing.

The check earns its place by what it replaces. A missing
`SeCreateTokenPrivilege` used to surface as an `EPERM` from the first
token mint, which is the first service start — registryd, in Phase 1
step 6. So a peinit that could not mint tokens entered recovery
reporting what read as a registryd problem, and nothing anywhere named
the privilege.

`SeImpersonatePrivilege` is **not required and not checked**, even
though it appears in the requirement list peinit was specified against.
It is not used. [*privilege.se-impersonate-is-not-required] peinit
passes the peer's token descriptor to AccessCheck directly rather than
impersonating the caller and evaluating as them, so the privilege that
would be needed to impersonate is not needed at all. The same holds on
the jobs socket: peinit never impersonates a submitter, and the identity
a job runs as is one the kernel already verified the submitter could
convey.

For non-platform services peinit creates no tokens. It installs the ones
authd minted. It mints only for the SYSTEM platform services it starts
during bootstrap, before authd is available.

> [!NOTE]
> Minting is interim. The intended model derives a service's token from
> peinit's own handle through a KACS duplicate-with-additions operation
> — kernel-attested as a descendant of peinit's real token, and needing
> no minting privilege. That would let peinit drop
> `SeCreateTokenPrivilege` entirely, which is the point: the privilege
> to mint arbitrary tokens is the strongest thing peinit holds and the
> one it has the least use for.
