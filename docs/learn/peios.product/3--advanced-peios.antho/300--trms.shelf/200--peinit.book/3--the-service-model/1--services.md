---
title: Services
description: The primary unit of management — names, the two service types, and how a forking daemon differs from a simple one.
---

A service is the primary unit of management: a definition in the
registry, a runtime state, a Security Descriptor, and at most one
running main process. Definitions live under
`Machine\System\Services\<name>`, where the key name *is* the service
name. [*svc.the-key-name-is-the-service-name] peinit reads them at Phase
2 boot and on an explicit reload-config.

## Names

A service name is 1 to 128 bytes drawn from `[A-Za-z0-9._-]`. Any other
byte makes the name invalid.
[*svc.a-name-is-1-to-128-bytes-of-the-name-alphabet]

Two exclusions are deliberate. `/` is out because names map directly
onto cgroup identifiers (§5.1) and onto registry key names, and a name
containing a separator would mean something different in each. `:` is
reserved for peinit's own synthetic naming.

## The two types

### Simple

A long-running daemon. peinit forks, installs a token, and execs the
binary; the process *is* the service. When it exits, the service has
stopped. [*svc.a-simple-services-main-process-is-the-service] This is
the default and covers nearly everything — registryd, authd, sshd,
application services.

Readiness comes from the `Readiness` field. `Notify`, the default,
waits for `READY=1`. `Alive` treats the process as ready the moment it
exists. [*svc.notify-waits-for-ready-and-alive-does-not]

The service goes Active on readiness and stays Active until the process
exits or something stops it.

### Oneshot

A run-to-completion task: database initialization, a schema migration, a
directory that has to exist. peinit forks, installs a token, execs, and
waits for the exit.

`Readiness` is ignored. [*svc.a-oneshots-readiness-field-is-ignored] A
Oneshot's readiness is always "it exited successfully", because
`READY=1` is meaningless from a process whose job is to finish. Success
means exit code 0, or any code listed in `SuccessExitCodes`.
[*svc.a-oneshot-succeeds-on-zero-or-a-listed-code]

The differences from Simple are:

- A successful exit goes to Completed. With `RemainAfterExit=1` it stays
  there; [*svc.remainafterexit-holds-a-completed-oneshot] without, it
  passes through Completed to release dependents and then goes Inactive.
- A non-zero exit goes to Failed. [*svc.a-nonzero-oneshot-exit-is-failed]
- `ExecStartPost` runs after the successful exit rather than after a
  readiness signal, and does not run at all if the Oneshot failed.
  [*svc.a-oneshots-post-hooks-run-after-a-successful-exit]
- `StartTimeout` covers the entire execution, from the first pre-hook to
  the process exiting.
  [*svc.a-oneshots-starttimeout-covers-the-whole-execution]

`RemainAfterExit` matters when the Completed state itself is the useful
information — so a status query shows a migration as finished rather
than as inactive.

## Forking daemons

peinit does not support them. A service that double-forks to daemonise
itself is working around a problem that does not exist when the service
manager tracks the process it spawned, and peinit tracks its child
through a pidfd obtained at fork.
[*svc.supervision-follows-the-forked-child] There is no `MAINPID=`, and
no way to point supervision at a different process.
[*svc.there-is-no-mainpid]

A legacy binary that insists on double-forking is wrapped by whoever
packages it — a script with a `--no-daemon` flag, typically. That is a
packaging concern.
