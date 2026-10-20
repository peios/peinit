---
title: The SYSTEM Path
description: peinit mints SYSTEM tokens itself, which is what breaks the bootstrap circle for registryd and authd.
---

For `Identity=SYSTEM`, peinit mints a token itself. This is what breaks
the bootstrap circle: registryd and authd need tokens, and authd — the
thing that mints tokens — is one of them. eventd and lpsd took this
path too until they moved to accounts of their own (`Identity=Service`);
they now take the authd path (§4.3) and start after authd.

## Minting [*token.minting-copies-peinits-own-token]

peinit reads its own token as a template and builds a new **primary**
token carrying the same identity: user SID `S-1-5-18`, the same group
list, the same privilege set, the same integrity level. The mint
requires `SeCreateTokenPrivilege`, which the boot SYSTEM token carries;
the kernel refuses the call with `EPERM` otherwise.
[*token.the-mint-requires-secreatetokenprivilege]

Two details of the copy matter.

**The logon session comes from the token's statistics.**
[*token.the-logon-session-comes-from-the-token-statistics] peinit takes
the `auth_id` from the source token's `TokenStatistics` — not from the
independent `interactivity_scope` field, and not from a hard-coded
well-known SYSTEM LUID. The minted token therefore stays associated
with the real SYSTEM logon session peinit was given at boot, while
carrying its own interactivity scope, which is zero for a platform
service. Substituting either of the other two values would associate
platform services with a session that does not exist.

**The logon SID group is dropped from the copy.**
[*token.the-logon-sid-group-is-dropped-from-the-copy] The kernel
re-appends the session's logon SID when it creates the token, and
rejects a create whose group list already contains it. So peinit filters
that group out of the template before building.

**Two groups are added that the template does not carry.**
[*token.the-service-sid-and-service-group-are-added] The
per-service SID (§4.4), which is what keeps platform daemons
distinguishable while they all run as SYSTEM; and the Service group
`S-1-5-6`, which every token authd mints for a service logon carries by
derivation from the logon type.

The second is load-bearing rather than cosmetic. `S-1-5-6` is the
grantee on anything reachable by services as a class — peinit's own
notification socket first among them (§10.5) — so a SYSTEM service
minted here without it would be unable to report itself ready, while an
otherwise identical service on an authd-minted token could. A service is
no less a service for having been started before the authority was.

**The minted token carries a default DACL the template cannot supply.**
[*token.the-minted-token-carries-the-system-default-dacl] A token's
default DACL is the DACL an object gets when the token creates it with
no parent to inherit from — an abstract socket, a key under a container
written without inheritable ACEs. The kernel has no fallback of its
own: a token whose default DACL is empty leaves such an object with a
null DACL, which grants everything to everybody. The bootstrap SYSTEM
token carries `D:(A;;GA;;;SY)(A;;GA;;;BA)` (see
[Bootstrap tokens](~peios/peinit/boot/bootstrap-identity)),
but a token's default DACL is not a field peinit can read back from its
own handle, so peinit states the same value itself and sets it on every
SYSTEM token it mints. Non-SYSTEM services do not pass through here; their
default DACL
is the authority's, from the `DefaultDacl` value of the principal's
policy record (§4.3).

peinit also asserts that its own token is a primary token and that its
user SID really is `S-1-5-18` before minting, and fails the start with a
message naming what it found otherwise.
[*token.the-template-must-be-a-primary-system-token] PID 1 minting from
something that is not the boot SYSTEM token is not a situation to
proceed from.

The minted token is fully independent. The privilege restriction that
follows (§4.5) operates on it alone and cannot affect peinit's own.
[*token.the-minted-token-is-independent-of-peinits]

> [!NOTE]
> Minting is an interim mechanism. The intended model is for peinit to
> *derive* the token from its own handle, through a KACS
> duplicate-with-additions operation — kernel-attested as a descendant
> of peinit's real token, and requiring no token-minting privilege at
> all. That operation does not exist in KACS yet, so peinit mints.
