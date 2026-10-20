---
title: Bootstrap Identity
description: The steady-state identity flow cannot start the system, so platform services run as SYSTEM until authd exists.
---

The steady-state identity flow is: peinit asks authd for a token, authd
mints it, peinit installs it on the child. That flow cannot start the
system, because authd needs a token of its own, and registryd has to be
running before it. The bootstrap model breaks the circle.

## Platform services run as SYSTEM

A service whose definition says `Identity=SYSTEM` gets a token peinit
mints from its own, with `kacs_create_token` (§4.2).
[*bootstrap.a-system-identity-is-minted-by-peinit] No authd interaction
is involved — which is the point, since authd does not exist when the
first of these services starts.
[*bootstrap.a-system-identity-does-not-consult-authd]

Two services use it:

| Service | Why |
|---|---|
| registryd | Starts before authd exists at all. |
| authd | Needs `SeTcbPrivilege` and `SeCreateTokenPrivilege`; it is the minter for everything else. |

lpsd and eventd used it too, until each moved to an account of its own
(`Identity=Service`). Both now require authd, start after it, and take
the authd path like any other service.

Nothing restricts which services can declare `Identity=SYSTEM`.
<!-- lint-disable-next-line spelling -->
[*bootstrap.no-allowlist-governs-who-may-be-system] There is
no allowlist, because an allowlist would be enforcing a boundary that is
already enforced somewhere better: the Security Descriptor on
`Machine\System\Services\`. Anyone who can create a service definition
is by definition trusted to choose its identity, and adding a second
list to maintain would only create a way for the two to disagree.

Every SYSTEM token peinit mints carries the service's per-service SID in
its group list, computed by peinit itself from the service name (§4.4).
[*bootstrap.a-minted-system-token-carries-the-per-service-sid] That is
what keeps platform services distinguishable to an access check despite
all of them running as `S-1-5-18`.

> [!NOTE]
> This is the same arrangement Windows uses: the SCM, LSASS, and the core
> platform services all run as LocalSystem. The trust anchor is that
> peinit *is* SYSTEM, and the kernel guarantees that by handing PID 1
> the boot token.

## After authd

Once authd is running, every subsequent service gets its token
through the ordinary authd flow (§4.3).
[*bootstrap.after-authd-every-token-comes-from-authd] A definition with
no `Identity` field defaults to `LocalService` — a well-known principal
with a minimal privilege set [*bootstrap.identity-defaults-to-localservice]
— and authd adds the per-service SID to the token it mints.
[*bootstrap.authd-adds-the-per-service-sid]
