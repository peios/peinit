---
title: The Boot and On-Demand Paths
description: The two ways a graph gets built and how they differ beyond scope — including the three places a bad definition can surface.
---

The two ways a graph gets built differ in more than scope, and the
differences are worth having in one place.
[*ondemand.the-boot-and-on-demand-paths-differ-beyond-scope]

| | Boot | On-demand |
|---|---|---|
| Members | Every boot-triggered root plus its closure | One requested service plus its closure |
| A missing hard-dependency target | Blocks the dependent | Blocks the dependent |
| A disabled hard-dependency target | Blocks the dependent | Blocks the dependent |
| A validation finding | The service is failed, others continue | The start fails |
| Multiple findings on one service | Only the highest-precedence one is reported | — |
| Conflicts | Two boot-triggered conflicting services fail both | The conflicting service is evicted |
| Context | One boot context | One context per explicit start |
| Failure of the whole build | Recovery mode | An error to the caller |

## Where a definition error lands

The three places a bad definition can be caught behave differently, and
which one catches it depends on what kind of wrong it is:

- **Decoding.** A definition that does not parse — an invalid name, a
  malformed trigger, an unclosed quote, a `registry:` check naming an
  uncacheable key, a duplicate field — is caught when the registry is
  read. It fails that service with `ValidationError` and the rest
  continue, at boot and on reload alike (§3.2).
<!-- lint-disable-next-line line-length -->
  [*ondemand.a-definition-that-does-not-parse-is-caught-when-the-registry-is-read]
- **Graph validation.** A definition that parses but does not fit —
  a cycle, a missing target, a flap-constraint violation, an invalid
  calendar expression — is caught here, and fails that service at boot
  or the whole reload on a reload-config (§7.2).
  [*ondemand.a-definition-that-does-not-fit-is-caught-at-graph-validation]
- **Start.** A definition that parses and fits but whose preconditions
  do not hold — a failed assert, an unresolvable identity, a missing
  binary — is caught when the service actually starts, and fails that
  activation (§5.2, §5.3).
  [*ondemand.a-precondition-that-does-not-hold-is-caught-at-start]

The dividing line between the first two is whether the problem is
visible in one definition on its own. Decoding sees one key at a time;
validation is the first place that can see two definitions together.
