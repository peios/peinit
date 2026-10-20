---
title: Conditions and Asserts
description: Two start-time checks in the same form over the same four check types, differing only in what a failure means.
---

Both are start-time checks in the same `type:argument` form, over the
same four check types. [*check.the-four-check-types] What differs is the
consequence of a failure.

| Check | Form | Passes when |
|---|---|---|
| path | `path:<path>` | The path exists, of any type. |
| file | `file:<path>` | A regular file exists there. |
| directory | `directory:<path>` | A directory exists there. |
| registry | `registry:<key>` | The registry key exists. |

**Conditions** describe when a service *applies*. A failed condition
skips the service: it transitions to Skipped, which satisfies its
dependents, [*check.a-skipped-service-satisfies-its-dependents] because
a service that does not apply has succeeded by not needing to run.

**Asserts** describe what a service *needs*. A failed assert fails the
service, with cause `AssertionError`
[*check.a-failed-assert-fails-the-service-with-assertionerror] — the
service was expected to run and a precondition it depends on is missing.

All entries of a kind are AND'd. [*check.entries-of-a-kind-are-anded]
Conditions are evaluated first; asserts only if every condition passed.
[*check.conditions-are-evaluated-before-asserts] Both are evaluated
before dependency resolution and before any pre-exec hook,
[*check.checks-precede-dependency-resolution-and-the-hooks] and an entry
with an empty argument or an unrecognized type is a decode error.
[*check.an-empty-argument-or-unknown-type-is-a-decode-error]

## Evaluation without blocking

peinit is single-threaded PID 1 and its event loop cannot block while a
check runs. Two constraints follow, and they are why the check types
behave differently from one another.

### Registry checks are cache-only

A `registry:` check is evaluated against the in-memory model, never by a
live registry read. Accepted paths are exactly `Machine\System\Services`,
`Machine\System\Init`, and `Machine\System\Services\<name>` with one
nonempty service-name component. Init descendants, deeper Services
paths, and trailing separators are decode errors, caught at load rather
than silently failing at start. The same rule applies to Conditions and
Asserts. [*check.a-registry-check-must-name-a-cached-key]

The two roots resolve as present. A
`registry:Machine\System\Services\<name>` check is true when that service
exists in the model; it does not read an arbitrary registry key. A
missing service is a valid check whose result is false.
[*check.a-services-registry-check-is-a-service-existence-test]

### Filesystem checks run in a helper

`stat()` can block uninterruptibly on hung I/O — a dead NFS mount, a
failing disk controller — so peinit does not call it from the event
loop. It forks a short-lived helper into a dedicated `checks/` cgroup
under the service's tree, using the same `clone3` path as any other
child. The helper stats the paths and reports over a non-blocking pipe;
peinit waits on the pipe and the helper's pidfd through epoll, and never
blocks.

`PreStartCheckTimeout`, default 5 seconds, bounds the helper. A check
that does not report in time is treated as **not satisfied**
[*check.a-check-that-does-not-report-in-time-is-not-satisfied] — the
fail-safe direction, so a condition skips the service and an assert
fails it. peinit then SIGKILLs the helper's cgroup and unregisters the
result descriptor, and the event loop is never held up by a hung check.

## When results are computed

Checks are evaluated once, before the service's dependencies start, and
the result is cached for the rest of that activation.
[*check.the-result-is-cached-for-the-activation] A service that waits a
long time for a dependency starts on the answer that was true when the
wait began, not on a fresh one.
