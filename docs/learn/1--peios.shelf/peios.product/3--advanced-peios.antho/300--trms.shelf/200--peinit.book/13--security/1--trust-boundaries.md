---
title: Trust Boundaries
description: The two boundaries peinit sits at — the kernel handing it a SYSTEM token, and peinit handing identity to services.
---

peinit sits at two.

## Kernel to peinit [*trust.the-kernel-hands-peinit-the-system-token]

peinit is the first userspace process, and the kernel gives it a SYSTEM
token — `S-1-5-18`, every privilege. This is the root of trust for all
userspace identity on the system.

peinit does not drop that token and does not authenticate to anything.
Its identity is axiomatic: there is no authority above it in userspace
that could vouch for it, and the kernel handing it the boot token *is*
the vouching.

## peinit to services

peinit creates service processes with specific identities and reduced
privileges. The trust runs one way. peinit trusts the kernel because it
has no alternative; services trust peinit because peinit gave them their
identity. Services do not trust each other — KACS mediates every
access between them, and nothing peinit does creates a relationship
between two services beyond the ordering their definitions asked for.

## The TCB

peinit is part of the Trusted Computing Base, alongside the kernel,
KACS, LCS, KMES, registryd, authd, lpsd and eventd. A compromise of any
of them compromises the system.

That list is not decoration. It is why registryd is exempt from the
global environment layer (§5.5) — a component in the TCB cannot be
configurable by a mechanism it is itself the enforcement point for — and
it is why eventd is Critical, since a TCB whose audit trail can be
silently stopped is not one.

## Filesystem enforcement

KACS enforces Security Descriptors on filesystem access. peinit's Phase
1 descriptor seeding (§2.3) exists precisely because of it: a freshly
mounted tmpfs carries no descriptor, and under `DENY_MISSING` every
inode on it would be unreachable to everything — including peinit —
until something stamps a descriptor it can inherit from.

That enforcement has no bypass. There is no owner exemption, no
privilege that overrides a missing descriptor, and no root escape
[*trust.descriptor-enforcement-has-no-bypass], which is what makes the
seeding step fatal on failure rather than advisory.

FACS extends the model further, and until it lands the filesystem layer
still relies partly on conventional trust — correct packaging,
controlled binary paths — for the objects nothing has stamped.
