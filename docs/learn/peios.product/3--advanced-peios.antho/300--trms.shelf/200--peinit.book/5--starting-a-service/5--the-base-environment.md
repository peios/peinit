---
title: The Base Environment
description: Every service environment is built from scratch in four layers, lowest precedence first, with nothing inherited.
---

peinit constructs every service and hook process's environment from
scratch, in four layers, lowest precedence first. Nothing is inherited
[*env.nothing-is-inherited-from-peinit]: peinit's own startup
environment holds `TERM` and nothing else (§2.1), and none of it is
passed through.

## Layer 1: the compiled-in base [*env.the-compiled-in-path]

One variable:

| Variable | Value |
|---|---|
| `PATH` | `/sbin:/bin` |

Executables are addressed through the root-level StrataFS runtime views.
Package storage paths under `/usr` are deliberately not on the default
search path.

<!-- lint-disable-next-line line-length -->
## Layer 2: global environment variables [*env.global-envvars-become-variables]

Each value under `Machine\System\Init\EnvVars\` becomes a variable: the
value name is the variable name, the `REG_SZ` data is the value. An
`EnvVars\PATH` overrides the compiled-in `PATH`; every other name adds.
[*env.an-envvars-path-overrides-the-compiled-in-one]

A malformed entry — an empty name, or a name containing `=` — fails the
whole layer, which at boot means recovery mode.
[*env.a-malformed-envvars-entry-fails-the-layer]

**registryd does not receive this layer.**
[*env.registryd-does-not-receive-the-global-layer] The exemption is a
trust rule, not an availability one. Write access to `EnvVars\` is equivalent
to compromising every service peinit starts: `LD_PRELOAD`,
`LD_LIBRARY_PATH` and their relatives are not filtered, because the
key's Security Descriptor is meant to be the control boundary. A key
that could inject into the daemon that enforces who can write it would
make that boundary self-referential.

The exemption is narrow, and matches on two things at once: the job's
resolved identity is `SYSTEM` and its service name is `registryd`.
[*env.the-exemption-matches-identity-and-name] A
non-platform service that happens to be called `registryd` receives the
ordinary layering. It matches on the *job*, so a hook of registryd's
running under a non-SYSTEM `HookIdentity` would receive the layer; and
it matches the resolved identity string, so a definition naming
`S-1-5-18` literally rather than `SYSTEM` would not be exempt.

registryd is launched in Phase 1, before `EnvVars` has been read at all,
so the exemption is only observable on a restart.

## Environment [*env.the-services-own-environment-overrides-the-layers-below]

The definition's `Environment` entries, overriding both layers below.

## Layer 4: protocol variables [*env.notify-socket-is-always-set]

`NOTIFY_SOCKET`, always. `LISTEN_FDS`, `LISTEN_FDNAMES` and
`LISTEN_PID`, only when descriptors are being injected — from the fd
store for a service, or from a submission's attachments for a submitted
job (§8.5). [*env.the-listen-variables-appear-only-with-injected-descriptors]

These have the highest precedence. The first three are inserted after
both configurable layers, and all four names are dropped from those
layers before insertion, so a service cannot override `NOTIFY_SOCKET`
and break its own notification protocol, and an `EnvVars\LISTEN_FDS=3`
cannot point an fd-store-less service's `sd_listen_fds` at whatever
happens to sit at descriptor 3.
[*env.the-protocol-names-are-filtered-out-of-the-configurable-layers]
The name filter is what protects the `LISTEN_*` set when it is *not*
being set.

`LISTEN_PID`, which a conforming `sd_listen_fds` implementation checks
against its own PID before trusting `LISTEN_FDS`, is appended by the
child itself after the clone, because only then is the PID known; the
name filter keeps a configured value from shadowing it.
[*env.listen-pid-is-appended-by-the-child]

## What peinit does not set [*env.no-home-user-logname-shell-or-term]

Not `HOME`, `USER`, `LOGNAME`, `SHELL`, or `TERM`. Peios identity is a
KACS token — a SID — rather than a passwd entry, so there is no
canonical home directory or sign-in shell to populate. A service that
needs one supplies it through `EnvVars\` or its own `Environment`; a
submitted job's submitter supplies it in the submission's
`environment`, which occupies layer 3 for a job.

## Hooks and probes [*env.hooks-and-probes-receive-the-identical-environment]

Hooks, health checks and reload commands are built through the same
path, so they receive the identical environment: the same layers, the
same `NOTIFY_SOCKET`, and the service's `WorkingDirectory`,
`LimitNOFILE`, `LimitCORE` and `RequiredPrivileges`. They never receive
`LISTEN_FDS` — stored descriptors go to the main process only.
[*env.hooks-never-receive-listen-fds]

## When changes apply [*env.changes-take-effect-at-the-next-start]

The global layer is a snapshot refreshed at boot and on reload-config,
and both it and the per-service `Environment` take effect at a service's
next start. Neither is applied to a running process.

> [!NOTE]
> Write access to `Machine\System\Init\EnvVars\` is equivalent to
> compromising every service on the system. peinit does not filter
> variable names, consistent with the registry's write-authority threat
> model — the key's descriptor is the boundary. The recommended default
> is SYSTEM full control and Administrators read-only.
