---
title: The Cgroup Tree
description: peinit uses cgroups v2 for exactly two things — knowing which processes belong to a service, and killing them all at once.
---

peinit uses cgroups v2 for exactly two things: knowing which processes
belong to a service, and killing all of them at once. It does no
resource accounting and sets no limits.
[*cgroup.no-accounting-and-no-limits]

Every service gets a tree: [*cgroup.every-service-gets-a-tree]

```text
/sys/fs/cgroup/peinit/<cgroup-id>/          service root
/sys/fs/cgroup/peinit/<cgroup-id>/main/     the main process
/sys/fs/cgroup/peinit/<cgroup-id>/hooks/    hooks and reload commands
/sys/fs/cgroup/peinit/<cgroup-id>/health/   health check invocations
/sys/fs/cgroup/peinit/<cgroup-id>/checks/   pre-start filesystem check helpers
```

The sub-cgroups satisfy cgroups v2's "no internal processes" rule, which
applies whenever controllers are enabled, and give hooks and probes
containment of their own so that killing one does not touch the service.

They are not all created at once.
[*cgroup.sub-cgroups-are-created-on-demand] The root and `hooks/` are
created when the first thing needs them — the first pre-exec hook, if
there is one — and `main/` and `health/` when the main process launches.

## The cgroup id [*cgroup.the-id-is-the-encoded-service-name]

`<cgroup-id>` is the service name with every byte outside
`[A-Za-z0-9._-]` percent-encoded as `%` plus two uppercase hex digits.
Service names are already restricted to that set (§3.1), so in practice
the id equals the name.

The encoding is a defensive guarantee that distinct names always map to
distinct, cgroup-safe ids. `%` is not itself in the safe set, so it
escapes to `%25` and the encoding is prefix-free. That is what makes it
injective, unlike a plain substitution of `/` for `-`, under which `a/b`
and `a-b` would collide.

The id is internal. The name a user sees is unchanged.

## Generations [*cgroup.a-leak-moves-the-service-to-a-fresh-tree]

A cgroup whose processes survived SIGKILL cannot be removed — `rmdir` on
it fails with `EBUSY`. peinit records the leak and moves the service to a
fresh tree:

```text
/sys/fs/cgroup/peinit/<cgroup-id>%gen<N>/
```

Old leaked trees persist until the next reboot.

Two conditions record a leak, and both are needed because they answer
different questions. [*cgroup.rmdir-ebusy-also-records-a-leak]
`cgroup.events` reporting `populated` at the post-kill deadline says live
processes remain. `rmdir` returning `EBUSY` says the tree cannot be given
up, which covers that case *and* the ones where nothing is running but
the directory still will not go. Acting on only the first left the second
reusing a tree that already existed and was not empty.

The generation advances **once per tree**, not once per leak recorded
against it. [*cgroup.the-generation-advances-once-per-tree] A single
failed start records two cleanup deadlines — one for
`hooks/`, one for the service tree — and a tree cleanup can report
`main/`, `hooks/`, `health/` and the root separately; all of those are
the same tree, so the first advances the generation and the rest find
themselves already behind it. Leaks are still deduplicated by path and
kind, so each is recorded once.

The separator is `%gen`, not `.gen`. `.` is in the safe set and service
names permit it, so a service literally named `app.gen1` would have
shared a directory with generation 1 of a service named `app`. `%`
self-escapes and the encoder only ever emits it followed by two uppercase
hex digits, so `%g` cannot occur inside an encoded id — which makes the
whole path injective, not only the id.
