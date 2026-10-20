---
title: Pre-Start Evaluation
description: Evaluating conditions and asserts before anything is forked — the two check mechanisms, and how results are cached.
---

Before anything is forked, peinit evaluates the service's conditions and
asserts (§3.5). The definition comes from the in-memory cache, so this
never touches the registry.

1. If the service names a `TTYPath` and another service is holding that
   terminal, it goes to Skipped with cause `TtyUnavailable` and the
   start is abandoned (§11.6).
   [*start.a-held-terminal-skips-before-the-checks] Asked first because
   a terminal is a fact about the machine right now, and evaluating
   conditions ahead of it would fork a check helper for a start that was
   never going to happen.
2. Conditions are evaluated. Any failure transitions the service to
   Skipped and abandons the start. Skipped satisfies dependents.
3. If every condition passed and the service has asserts, they are
   evaluated. Any failure transitions the service to Failed with cause
   `AssertionError` and abandons the start.

Only when all three pass does the pre-exec sequence continue.

## Where the transition happens [*start.the-evaluation-gates-the-transition]

For a fresh start from Inactive, the evaluation gates the transition:
the service becomes Starting only after the checks pass, so a skipped
service goes straight Inactive → Skipped.

For an activation that is already Starting — the start leg of a restart,
for instance — the transition has already happened, and the evaluation
gates further progress instead. A restart whose conditions do not
hold therefore passes through Starting on its way to Skipped, where a
fresh start would not.

An assert failing takes the same route to Failed on both paths, and on
both it propagates: dependents in the same graph context hear that the
dependency failed (§7.3). This matters more on the restart leg than
elsewhere, because an `AssertionError` is never restarted — where a
running dependent is deliberately unaffected by a dependency *crashing*
(§7.1), since a crash is transient and the restart policy handles it, a
failed assert means the dependency is not coming back without
intervention.

## Two check kinds, two mechanisms

`registry:` checks resolve against the in-memory model. Since a
non-cacheable key is rejected when the definition is read, this
evaluation never needs a live read.

Filesystem checks — `path:`, `file:`, `directory:` — run in a forked
helper. peinit clones it with `CLONE_PIDFD | CLONE_INTO_CGROUP` into the
service's `checks/` sub-cgroup, exactly as it launches anything else.
The helper stats the paths and writes the results to a non-blocking
pipe; peinit watches the pipe and the helper's pidfd through epoll.

The terminal is re-checked when the helper's results come back, rather
than the earlier answer being carried over
[*start.the-terminal-is-rechecked-after-the-helper]: the helper ran in
between, and a terminal can change hands while it did.

`PreStartCheckTimeout` bounds the helper, at 5 seconds by default. On
expiry every check still outstanding is marked **not satisfied** — the
fail-safe direction — and re-evaluated on that basis, so a condition
skips the service and an assert fails it. peinit then kills the helper's
cgroup and unregisters the result descriptor.

A helper that survives the kill, stuck in uninterruptible sleep, has its
cgroup abandoned and recorded exactly as a leaked hook or health cgroup
is (§5.7).

## Caching

The results are computed once, before the service's dependencies are
started, and reused for the rest of that activation. A service that
waits a long time on a dependency starts on the answer that was true
when the wait began.

Filesystem checks are gathered in one helper run, from the conditions
and the asserts together. [*start.one-helper-run-covers-both-lists]
There is only ever one run — the completion
path evaluates both lists against the results it gets back, and there is
no mechanism to ask for a second — so both lists' paths have to be
stat'd before it. A path named in both is stat'd once.

A check with no result counts as not satisfied, which is the fail-safe
direction for a timeout and is why the gather has to be complete: an
omitted path is indistinguishable from a path that is not there.
