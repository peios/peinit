---
title: The authd Path
description: Every non-SYSTEM token comes from authd — what peinit sends, what it must never do to obtain one, and why the token it gets back is deliberately weaker.
---

For any identity other than `SYSTEM`, the token comes from authd.
[*token.a-non-system-identity-comes-from-authd] peinit
does not resolve identities, does not know whether a principal is local
or from a domain, and does not want to: routing is authd's whole
purpose.

The division is not that peinit lacks the privilege to mint — it plainly
has it, and uses it for the `SYSTEM` path (§4.2). It is that the
privilege set and integrity level an identity carries are **policy**,
and policy is authd's. A second component deciding it in parallel is a
disagreement waiting to happen, and peinit has already shipped one: its
hand-written privilege bit table had four wrong entries, silently
stripping privileges a service had asked to keep.

## The request [*token.the-request-is-a-serviceattest-on-the-logon-socket]

peinit sends a `ServiceAttest` on `/run/logon.sock`, specified as PGSS
Logon §2.19. It carries two strings:

| Field | Value |
|---|---|
| `identity` | The definition's `Identity` value, verbatim |
| `service` | The name of the service being started |

authd answers with `AccessGranted` carrying the token as an `SCM_RIGHTS`
descriptor, or `AccessDenied`. There is no credential exchange, because
a service identity has no credential and must never acquire one — a
machine that could authenticate its own services unattended would have
to hold their secrets.

authd derives the per-service SID from `service` and adds it to the
token, so the SID that distinguishes one service from another comes from
the authority on this path rather than from peinit (§4.4).
[*token.authd-adds-the-per-service-sid] Note that
authd cannot verify `service`: the process it names does not exist yet,
so there is no token to interrogate. peinit is trusted on it, which is
the reason for the restriction below.

<!-- lint-disable-next-line line-length -->
## This request must come from PID 1 [*token.a-service-attest-must-come-from-pid-1]

authd authorizes a `ServiceAttest` on two facts: that the peer's token
names SYSTEM, and that **the peer is PID 1**. The second is not
redundant. SYSTEM alone is every platform daemon on the machine, so
without it, compromising any one of them would yield a token for any
service identity authd would mint.

That makes it a standing constraint on this crate: the connection is
opened by peinit's own process and must never be opened by a forked
helper. Nothing in the code enforces it. If it is ever broken the
symptom is every non-SYSTEM service failing to start at boot with an
authorization error, which reads like a fault in authd rather than like
a refactor.

<!-- lint-disable-next-line line-length -->
## The token is weaker than a user's, deliberately [*token.an-attested-token-is-impersonation-not-delegation]

An attested token is minted one rung lower on the impersonation ratchet
than a credentialled logon: `Impersonation` rather than `Delegation`.

The evidence behind it does not travel. A credential a principal source
verified is something another machine could in principle have checked
too; peinit's word is evidence on this machine only. A service token
that could be forwarded off the box on the strength of it would let one
system's PID 1 act as an arbitrary account on another.

Because the level is a ratchet, nothing derived from a service's token
can exceed it either.

## Failure is a failure [*token.an-unreachable-authority-fails-the-start]

Every non-SYSTEM service start depends on authd being reachable, and
every one of them fails if it is not. Only services using the SYSTEM path are
unaffected; platform services
such as eventd and lpsd use the authority too.

peinit *orders* around that dependency rather than merely suffering it.
The authority declares `Provides = ["authn"]`, and peinit derives a hard
dependency on whoever fills that role for every service whose `Identity`
— or whose `HookIdentity`, where it has hooks — is not `SYSTEM`
([§7.6](~peios/reference/peinit/dependencies/derived-dependencies)).
Nothing is written on the consumer side: the edge comes from the same
field that creates the requirement, so a new service cannot forget it.

Until that existed the ordering was left to each definition, and all
three `LocalService` definitions in the tree omitted it — so a service
needing a token routinely reached this code before the socket was bound
and failed its launch with `ENOENT`, on most boots, recovering only
through its restart policy.

This is the intended behavior rather than a limitation. The client that
preceded it returned a freshly minted SYSTEM token whenever it could not
do better, so a service declaring `LocalService` ran with peinit's entire
privilege set — and status output reported it as `LocalService`, which is
what kept the problem invisible from outside. A service that does not
start is visible; a service running as the wrong principal is not.

`summarize_token` now refuses to describe a token as an identity it does
not carry: where the declared identity predicts a user SID, a token
carrying a different one fails the launch rather than being reported
under the name that was asked for.
[*token.a-token-contradicting-the-declared-identity-fails-the-launch] It
is only checked where the identity
predicts a SID — a principal name is authd's to resolve, and peinit has
nothing to compare it against.

<!-- lint-disable-next-line line-length spelling -->
## What can be attested [*token.the-platform-service-identities-may-be-attested]

authd accepts the platform's own service identities — `SYSTEM`,
`LocalService`, `NetworkService`, and the per-service virtual account
selected by `Service` — which no principal source holds and
for which no credential could exist.

Anything else must be designated in the principal's own record as usable
for a service logon, and until that designation exists authd refuses it.
So a definition naming an ordinary principal does not start.
[*token.an-undesignated-principal-is-refused] That is the
property the design exists to guarantee: without it, the service manager
would be an oracle that mints a credential-free token for anybody on the
machine.

<!-- lint-disable-next-line line-length -->
## The service's own virtual account [*token.service-selects-its-own-virtual-account]

`Identity=Service` asks authd to use the per-service SID (§4.4) as the
**user SID**, as well as including it in the token's groups. The spelling
`Service` is case-insensitive. Each service name therefore selects a
separate virtual account without creating an account in a principal source
or enabling a credentialled logon.

The derived groups include Everyone (`S-1-1-0`), Authenticated Users
(`S-1-5-11`), Local (`S-1-2-0`), and Service (`S-1-5-6`), together with the
kernel's per-session logon SID. It does not acquire SYSTEM or LocalService
membership. Privileges, integrity, owner, and default DACL still come from
authd's policy evaluation; choosing a distinct user SID grants no extra
privileges by itself.

With `Identity=LocalService`, the user SID is the shared `S-1-5-19` instead.
The service's own SID remains a group, so ACLs can still distinguish two
services that share that account. Use `Service` when their user identities
should also be distinct.

`Service` takes the same derived hard dependency on the providers of the
`authn` role as other non-SYSTEM identities (§7.6). On the standard platform
that provider is authd; an explicit `Requires=authd` is unnecessary.
[*token.service-orders-after-the-authority]

## The interface is authd's

peinit speaks the protocol through `libauthd`, which is its definition,
rather than encoding the message itself. A second implementation of a
wire format that mints identity is the kind of duplication that fails
silently and in the worst place.
