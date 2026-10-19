---
title: Derived Dependencies
description: Provides names a role, and peinit derives the dependency a service's Identity implies — why the edge is derived rather than declared, and why a role is not an online target.
---

Most dependencies are declared. One is not.

A service whose `Identity` is anything but `SYSTEM` cannot start until the
authority is *listening*, because peinit has to ask it for a token before
there is a process at all
([§4.3](~peios/advanced-peios/peinit/service-identity/the-authd-path)).
That is a hard dependency in every sense
[`Requires`](~peios/advanced-peios/peinit/dependencies/relationships)
means one.

It used to be nobody's. peinit hardcodes the socket the authority listens
on, so the requirement was real on every boot, but the *ordering* existed
only if a definition happened to declare it — and all three
`LocalService` definitions in the tree did not. Three of three is the
number that makes it a defect in the mechanism rather than in three
definitions: the requirement is created by `Identity`, so `Identity` is
what should produce the edge.

## Provides

A service names the roles it fills:

```text
Provides = ["authn"]
```

A **role** is a virtual name — a capability, not a service. Virtual and
real names share one namespace, exactly as they do for packages (PSPU
§5.4): a dependency on `authn` is satisfied by a service literally called
`authn`, or by any service providing it.
[*derived.a-real-service-name-and-a-role-share-one-namespace]

The indirection is the point. peinit knows it needs *an* authority and
knows which socket to speak to, but the authority's **name** is registry
data and none of peinit's business — an image can ship a different one.
So the derived edge names the role, and peinit resolves it to whoever
declares it.

`Provides` entries use the service-name grammar and never carry a
readiness level. `netd:routed` names a condition *on a service*, and a
role is not a service, so there would be nothing for the level to
qualify; a level on a `Provides` entry is rejected rather than ignored.
[*derived.a-level-on-a-provides-entry-is-rejected]

The *other* side of a dependency can name a role, with or without a
level. `Requires = ["network:routed"]` is rewritten to one entry per
service providing `network`, the level carried across, before the
definition set becomes a service table — the same pass, at boot and on
every reload, that derives the edges below.
[*derived.a-declared-role-reference-is-rewritten-at-boot-and-on-every-reload]
`Requires`, `Wants`, and
`BindsTo` are rewritten;
[*derived.requires-wants-and-bindsto-are-all-rewritten] a real service
name is never mistaken for a role,
[*derived.a-real-service-name-wins-over-a-role-of-the-same-name] a
provider never gains a dependency on its own role, and a role no
service fills is left as written for validation to report. See
[§7.5](~peios/advanced-peios/peinit/dependencies/readiness-levels) for
why the network is declared this way.

## What peinit derives

Before a definition set becomes a service table — at boot and on every
reload, through the same code, because an edge that held until the first
reload would be worse than no edge — peinit adds a `Requires` on each
provider of `authn` to every service that needs the authority to launch.
<!-- lint-disable-next-line line-length -->
[*derived.a-service-needing-the-authority-gains-a-requires-on-each-authn-provider]

A service needs it when its `Identity` is not `SYSTEM`, **or** when it has
hooks and its `HookIdentity` is not `SYSTEM`.
[*derived.hooks-running-as-a-non-system-identity-also-need-the-edge] The
second half matters: a
`SYSTEM` service whose hooks run as `LocalService` materialises a token
for them, and a definition that needs the authority for half its launch
needs the ordering for all of it.

Three rules keep the derivation from doing damage:

- **A provider never gains a dependency on its own role.**
  [*derived.a-provider-never-gains-a-dependency-on-its-own-role] An
  authority declaring a non-SYSTEM identity would otherwise require
  itself, and a service that requires itself never starts.
- **An edge already declared is not added again**, whether it was
  declared as `Requires` or `BindsTo`, and including one carrying a
  level. [*derived.an-already-declared-edge-is-not-added-again]
  `Requires = ["authd:ready"]` already orders against the
  authority, and more strictly; adding a plain edge beside it would be a
  second edge to the same service saying less. `BindsTo` requires its
  target as well as binding to it, so the same holds for it.
- **Nothing is derived when no service fills the role.**
  [*derived.nothing-is-derived-when-no-service-fills-the-role] See below.

Because the result is an ordinary `Requires`, it needs no new kind of
edge and behaves exactly like a declared one everywhere else — in
[graph
validation](~peios/advanced-peios/peinit/dependencies/graph-validation),
in [graph
execution](~peios/advanced-peios/peinit/dependencies/graph-execution),
at shutdown, and in `svctl status`, where it appears as the dependency it
is rather than as a hidden ordering.
[*derived.the-derived-edge-behaves-exactly-like-a-declared-requires]

Several services MAY fill one role; each becomes its own `Requires`, so
the dependent is ordered after all of them.
[*derived.every-provider-of-the-role-becomes-its-own-requires] For
`authn` this cannot
arise — PGSS Logon §2.1 requires at most one authority on a running
system — and where a future role allows several, ordering after all of
them is the conservative reading and needs nothing new. peinit
deliberately does not offer "wait for any one of them": that would change
the release rule from *every edge satisfied* to *every group has one
satisfied*, and would contradict `Requires`'s failure rule, under which a
target entering Failed fails the dependent.

## When nothing fills the role

An image whose services need a token and which ships no authority is
broken. A reload of it produces a validation **warning** naming every
service that cannot start, and succeeds anyway.
[*derived.an-unfilled-role-is-a-warning-and-the-reload-still-succeeds]

It deliberately does not invent an edge to a name no service answers to.
That would be a missing hard dependency, and a missing hard dependency
rejects an entire reload transaction — so an operator could not reload
the definition that installs the authority. The image would be
unrecoverable by the one mechanism meant to recover it.

The individual launches still fail, loudly, where the fault actually is:
at the service that asked for a token and could not have one.
[*derived.the-launch-that-cannot-get-a-token-is-what-fails]

## A role is not an online target

[§7.5](~peios/advanced-peios/peinit/dependencies/readiness-levels) opens
by rejecting `network-online.target`, and the objection there does not
reach this mechanism. It is worth being explicit about why, because the
two look alike from a distance.

The complaint about `network-online.target` is that **"online" has no
single definition**: a service needing a default route and a service
needing a loopback address want different things, and one name cannot
mean both. The abstraction is over a *condition*, and the condition is
ambiguous.

`authn` abstracts over no condition at all. It means "whoever answers
`ServiceAttest` on the logon socket" — a protocol role, specified by PGSS
Logon §2.19, which §2.1 says MUST have at most one occupant. There is
exactly one thing it can mean, and what a dependent gets is the same
thing it would get by naming the service directly.

`network:routed` passes the same test from the other direction. The role
`network` is a position too — the executor of Peios Network Policy's
interface layer — and `routed` is not peinit's word for "online" but the
specification's word for one exact condition, a default route. The
ambiguity that sinks `network-online.target` is absent because the
condition is named, not the target.

The rule the two share: peinit does not invent vocabulary for conditions
inside another service. A readiness level is the publisher's own word for
its own state, or its specification's; a role is a name for a *position*
in a protocol peinit already speaks. Neither is a target.
