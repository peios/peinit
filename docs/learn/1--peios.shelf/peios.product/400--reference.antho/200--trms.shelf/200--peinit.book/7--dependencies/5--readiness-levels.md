---
title: Readiness Levels
description: How a service waits for a condition inside another service — Requires = ["netd:routed"] — and how publishers report a set of current conditions.
---

`network-online.target` on systemd is famously meaningless, because
"online" has no single definition. A service needing a default route and
a service needing only a loopback address want different things, and one
target cannot mean both.

peinit does not have an online target. A service says what it is waiting
for, in the publisher's own vocabulary:

```text
Requires = ["netd:routed"]
Wants    = ["timed:synchronised"]
```

`netd:routed` waits for netd to be active **and** to have published the
level `routed`.
[*ready.a-level-dependency-waits-for-the-level-as-well-as-the-service]
`netd` on its own is unchanged — the ordinary "that service must be
active" dependency.

## The syntax

`<service>:<level>`, split on the first colon.
[*ready.a-declared-target-splits-on-the-first-colon] It rides on the
existing `Requires`, `Wants` and `BindsTo` fields rather than a field of
its own, because a level dependency *is* a service dependency with a
stricter predicate: `netd:routed` subsumes `netd`. Each relationship
keeps exactly the semantics
[it already has](~peios/reference/peinit/dependencies/relationships),
applied to
the stricter test. [*ready.each-relationship-keeps-its-semantics-with-a-level]

It cannot collide with a service name. A service name can contain only
letters, digits, `.`, `_` and `-`, so a colon never appears in one, and
no existing definition can accidentally become a level dependency.
[*ready.a-service-name-can-never-contain-a-colon]

A trailing colon — `"netd:"` — reads as a plain dependency on `netd`.
[*ready.a-trailing-colon-is-a-plain-dependency] It
is a typo, and treating it as a request for the empty level would produce
a condition nothing could ever satisfy.

## Through a role

The service side of the syntax can be a role instead of a name:

```text
Requires = ["network:routed"]
```

`network` is filled by whichever service declares `Provides = ["network"]`
— netd, on a shipped image — and peinit rewrites the entry to
`netd:routed` before anything reads it, exactly as
[§7.6](~peios/reference/peinit/dependencies/derived-dependencies)
resolves a role. The level rides across unchanged.
[*ready.a-role-carrying-a-level-is-rewritten-to-the-provider-with-that-level]
Everything downstream
— validation, the graph, `svctl status` — sees a level dependency on a
service, and never learns a role was involved.

This is the form a definition should use for the network. The role is
"the executor of Peios Network Policy's interface layer", and the levels
are not the executor's own words but the specification's: PNP defines
readiness as `link`, `addressed` and `routed`, and whichever service
fills the role publishes those. A definition that says `network:routed`
survives netd being replaced; one that says `netd:routed` does not.

Several services can fill a role; the entry then resolves to one level
dependency per provider, and a `Requires` waits for all of them.
[*ready.a-role-with-several-providers-becomes-one-entry-per-provider] A
role no service fills is left as written and fails validation as the
missing hard dependency it is, naming the role the definition wrote.
[*ready.an-unfilled-role-fails-validation-naming-the-role]

## How a level gets there

The service publishes it on the notification channel it already has, as
`LEVEL=` beside `READY=1` and `STATUS=`:

```text
LEVEL=link
LEVEL=addressed
LEVEL=routed
```

All `LEVEL` lines in one datagram form one set: peinit atomically replaces
the sender's previous set with the distinct nonempty values, before
releasing any dependents, including when the same datagram carries
`READY=1`. A datagram with no `LEVEL` lines leaves the set unchanged.
[*ready.a-datagram-replaces-the-whole-level-set]

`LEVEL=` on its own clears the set. Empty values mixed with nonempty
ones add no member; they do not clear other lines in the same datagram.
[*ready.an-empty-level-retracts-it] peinit records the set against the
sending service, and holds any dependent whose requested level is absent.
<!-- lint-disable-next-line line-length -->
[*ready.a-level-is-recorded-against-the-sender-and-holds-a-mismatched-dependent]

There is no subscription. peinit does not connect to netd's socket or
timed's, does not need to find them before they exist, and carries no
knowledge of their wire formats
[*ready.peinit-never-connects-to-a-publishers-own-socket] — which
matters, because PID 1 is the one process on the machine that cannot
afford to fail to start.

## Levels match set membership

`Requires = ["netd:addressed"]` is satisfied when the active publisher's
set contains `addressed`. Every member is an opaque, case-sensitive name;
commas do not split a name into several levels.
[*ready.a-level-matches-membership-in-the-published-set]

peinit has no ordering over another daemon's vocabulary. A publisher
that sends only `LEVEL=routed` does not satisfy an `addressed` dependent.
netd knows which conditions imply others, so when routed it publishes
`link`, `addressed` and `routed` together. A service should depend on the
condition it needs, and the publisher reports every condition that holds.

## A level is never stale

peinit clears a service's entire level set whenever that service leaves
a state that satisfies dependents.
[*ready.a-level-is-cleared-when-its-publisher-stops-satisfying-dependents]
A level is a claim about a process that is
making it, and one that outlived its process would hold a
dependent open on a promise nobody was keeping — the exact failure the
mechanism exists to prevent.

Because peinit is the process that notices a service exiting, this needs
no timeout and no liveness check.

## How the gate holds

A level is settled at dispatch, not at planning. The plan decides *what*
to start; whether a level is met is a live question — it can arrive after
the target's own start completed (netd is active long before DHCP
finishes) and be retracted while a dependent is still waiting — so the
[execution graph](~peios/reference/peinit/dependencies/graph-execution)
re-checks it against the service table every time it considers releasing
the dependent, and holds the start until the answer is yes.
<!-- lint-disable-next-line line-length -->
[*ready.a-level-is-re-checked-at-every-release-rather-than-settled-at-planning]

That check runs even when there is nothing to start. An already-active
target is not part of the start plan, but the condition on it still is:
the graph keeps a level edge to a service it is not starting, which is
what makes `Requires = ["netd:routed"]` hold when netd is up and merely
not routed yet.
[*ready.a-level-edge-is-kept-to-a-target-that-is-not-being-started]

Two events re-open the question. A level-set update arriving from the
publisher releases every dependent whose requested level is now present
— this is the only thing that can open a `Requires` or `BindsTo` gate,
which take the exact level and nothing else.
[*ready.only-the-level-itself-opens-a-hard-gate] And the
publisher leaving a state that satisfies dependents
releases `Wants` waiters: a soft dependency waits only while someone is
running who could still publish the level, and proceeds once nobody is —
which keeps `Wants` failure-tolerant, the property that defines it.
[*ready.a-publisher-leaving-a-satisfying-state-releases-wants-waiters]

A held start does not time out, and its `StartTimeout` runs from the
moment the level arrives, not from when the start was asked for.
`Requires = ["netd:no-such-level"]` holds the dependent indefinitely:
the service stays inactive and the start operation stays pending,
visible in `svctl status` as an operation that has not completed.
[*ready.a-held-start-does-not-time-out] That is the declared semantics,
not a hang — the condition was never met, so the start never happened.

## What happens on a drop

Nothing, in this version. If a level falls away after a dependent has
already started, the dependent keeps running and is not told.
[*ready.a-level-dropping-after-the-start-does-nothing]

That is a deliberate limit, not an oversight. The notification channel is
one-way by specification (PSPU §4.16), so peinit has no way to tell a
running service anything. A service that needs to react to a level
changing while it runs should subscribe to the publisher directly — which
is what resolvd does with netd today, and what any consumer that cares
that much will want anyway, since it will need the detail rather than
the fact.

peinit's contribution is the start gate: the moment when the service does
not exist yet and so cannot subscribe to anything at all.

## Published levels [*ready.netd-and-timed-are-the-shipped-publishers]

| Publisher | Levels |
|---|---|
| netd, as the role `network` | `link` → `addressed` → `routed`, defined by Peios Network Policy |
| timed | `unsynchronised`, `settling`, `synchronised`, `spike` |

Any service can publish a level; nothing about the mechanism is specific
to these two. The vocabulary is the publisher's own — or, where the
publisher fills a role a specification defines, the specification's —
and needs no registration.
