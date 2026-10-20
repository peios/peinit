---
title: The Execution Environment
type: concept
description: Configure a service's environment, working directory, runtime paths, hooks,  and terminal without losing logs or widening access.
related:
  - peios/services-and-jobs/defining-a-service
  - peios/services-and-jobs/identity-and-privileges
  - peios/services-and-jobs/the-service-lifecycle
  - peios/services-and-jobs/output-and-logging
---

If a program works in your shell but fails as a service, compare its
**identity**, **environment**, **working directory**, and **standard
streams**. peinit does not inherit your logon session. It launches the program
with the context in its definition.

Start with `svctl definition show <service>`, then inspect
`svctl status <service>` and the [service's
logs](~peios/services-and-jobs/output-and-logging). Check hook output as well
as the main program. Environment and working-directory changes reach the next
process start; they do not change a process already running.

## The environment

peinit constructs each service's environment in layers, lowest precedence
first — it does **not** pass through its own near-empty startup environment.

1. **Compiled-in base.** A fixed floor peinit always provides —
   `PATH`:
   `/bin`.
2. **Global `EnvVars`.** Each value under `Machine\System\Init\EnvVars\`
   becomes a variable (value name → variable name). `EnvVars\PATH` overrides
   the base `PATH`; other names add. Every service gets this layer except
   **registryd**, which serves the key itself — see the warning below for why
   that exemption exists.
3. **Per-service `Environment`.** The definition's own `KEY=VALUE` entries,
   which override layers 1–2.
4. **Protocol variables.** `NOTIFY_SOCKET` (always) and `LISTEN_FDS` /
   `LISTEN_FDNAMES` (only when [stored fds](#the-fd-store) are injected).
   These have the **highest** precedence and cannot be overridden — a service
   that clobbered `NOTIFY_SOCKET` would break
   [sd_notify](~peios/services-and-jobs/service-types).

A change to `EnvVars\` takes effect on a service's *next start*, like the
per-service `Environment` field — it is not pushed into running services.

peinit deliberately does **not** set `HOME`, `USER`, `LOGNAME`, `SHELL`,
or `TERM`. Peios identity is a
[token](~peios/services-and-jobs/identity-and-privileges) (a SID), not a
passwd entry, so there is no canonical home directory or sign-in shell to fill
in. A service that needs any of these supplies it through `EnvVars\` or its
own `Environment`.

> [!WARNING]
> Write access to `Machine\System\Init\EnvVars\` is equivalent to compromising
> **every service peinit starts** — a principal who can set it can inject
> `LD_PRELOAD`, `LD_LIBRARY_PATH`, and the like into all of them. peinit
> deliberately does not filter variable names; the key's security
> descriptor is the control
> boundary. Keep it tight — the recommended default is SYSTEM full,
> Administrators read-only.
>
> This is also why **registryd** is the one service that never receives this
> layer: it is the daemon that *serves* the key. Letting the key inject into
> the process that enforces who can write it would make the control boundary
> self-referential. Every other service, platform daemons included, gets the
> ordinary layering.

## Working directory and limits

| Field | Effect |
|---|---|
| `WorkingDirectory` | The process's working directory. Must be a non-empty absolute path; defaults to `/`. |
| `LimitNOFILE` | `RLIMIT_NOFILE` — the maximum number of open file descriptors. |
| `LimitCORE` | `RLIMIT_CORE` — the maximum core-dump size, in bytes. |

peinit also sets `oom_score_adj`: `-1000` (OOM-immune) for
`ErrorControl=Critical` services, and the default `0` for everyone else — so
the kernel never picks a Critical platform daemon as an out-of-memory victim.

## Runtime directories

`RuntimeDirectories` gives a service private scratch space under `/run`
without an `ExecStartPre` that has to `mkdir` it by hand. Each name you list
becomes a directory created **directly under `/run` ** —
`RuntimeDirectories=myapp` yields `/run/myapp` — set up immediately before the
main process starts. Each one gets a security
descriptor granting full access to
SYSTEM, Administrators, and the service's own
[SID](~peios/services-and-jobs/identity-and-privileges), so the service can
write to its directory while other principals cannot. If a directory can't be
created, or its descriptor can't be applied, the start fails with
`ParentSetupFailure`.

peinit does **not** remove these directories when the service stops. `/run` is
a boot-scoped tmpfs, so they disappear at the next boot rather than
being torn down on each stop — a restarting service finds its runtime
directory (though not necessarily its contents) already in place.

For the exact naming rules on each entry and the field's type and default, see
the [Registry key reference](~peios/services-and-jobs/registry-key-reference).

## Standard streams

peinit wires a service's standard streams before exec:

- **stdin** is redirected to `/dev/null`. peinit never gives a service an
  interactive input channel; a service that needs input must obtain it
  explicitly (a socket, a stored fd).
- **stdout** and **stderr** are redirected to pipes peinit holds, so it can
  capture and forward every line. This is the subject of [Service output and
  logging](~peios/services-and-jobs/output-and-logging).

### Attaching a terminal

A service that sets **`TTYPath`** to an absolute terminal path —
`/dev/console`, `/dev/tty1`, a serial line — gets that terminal on all three
standard streams instead of the wiring above, and becomes a session leader
owning it as its **controlling terminal**. That last part is what makes job
control work: Ctrl-C interrupts, `fg` /`bg` behave, and a hangup reaches the
process group. It is what a boot shell or a `login` prompt needs.

It carries a path rather than a flag because which terminal is a per-service
question: a maintenance shell on `tty1` while the kernel console is a serial
line is an ordinary thing to want.

> [!WARNING]
> Attaching a terminal **suppresses log capture** — the pipes eventd would
> read are closed, so the service's output goes to the terminal and nowhere
> else. Don't set `TTYPath` on a service whose output you expect to find in
> the logs.

Two services pointed at the same terminal do not fight over it. peinit gives
the device to one of them and **skips** the rest — highest `TTYPrecedence`
wins — and a skipped service can ask to be started when the terminal frees,
with the `tty:released` trigger. See [Queueing for a
terminal](~peios/services-and-jobs/triggers-and-timers).

## Pre-exec and post-exec hooks

Hooks let a service do work *around* its main process without baking it into
the binary.

- **`ExecStartPre`** runs *before* the main binary. Each command runs in
  sequence, in the `hooks/` sub-cgroup; **any** hook exiting non-zero aborts
  the start (the whole cgroup is killed and the service Fails with
  `PreHookFailure`). Use it for prerequisites that must hold before the
  service starts — creating a runtime directory, waiting on a precondition.
- **`ExecStartPost`** runs *after* readiness (Simple) or *after a successful
  exit* (Oneshot). A post-hook failure is **logged but does not fail the
  service** — by the time it runs, the service is already up. (A Oneshot that
  *fails* never runs its post-hooks.)

Hooks run under
[`HookIdentity`](~peios/services-and-jobs/identity-and-privileges) if set,
otherwise the service's own identity. Their output is captured and tagged like
the main process's, labeled with the hook (for example
`jellyfin/ExecStartPre[0]`).

### Command string parsing

The command fields — `ExecStartPre`, `ExecStartPost`, `ExecReload`,
`HealthCheck` — are parsed into an argument list with simple, predictable
rules. **peinit never invokes a shell.**

- Whitespace splits the command into arguments; double quotes group text
  containing spaces into one argument and are not retained (`--name="hello
  world"` → one argument `--name=hello world`).
- There is **no** shell expansion, variable substitution, or globbing.
  Backslash is literal; a single quote is an ordinary character.
- The first argument is the executable and **must be an absolute path**
  beginning with `/`. Relative names and PATH search are validation errors.
- An empty or whitespace-only command, or an unclosed quote, is a validation
  error.

If a command genuinely needs shell features, wrap it in a script and point the
field at the script.

`ExecReload` can also be a signal, written `signal:<NAME>` — for example
`signal:SIGHUP`. The name must be an exact, canonical Linux signal name
(uppercase, no aliases, no numbers); `SIGKILL` and `SIGSTOP` are rejected
because they cannot be handled as a reload. With no `ExecReload` at all,
reload sends `SIGHUP`. The reload lifecycle is covered in [Controlling
services](~peios/services-and-jobs/controlling-services).

## Conditions and asserts

Before peinit runs any hook or the main process, it evaluates **conditions**
and **asserts** — start-time checks in the same `type:argument` form. The
difference is what failure *means*:

- A failed **Condition** *skips* the service — it transitions to
  [Skipped](~peios/services-and-jobs/the-service-lifecycle), which **satisfies
  its dependents** (it succeeded by not needing to run). Conditions express
  "only run here if it makes sense."
- A failed **Assert** *fails* the service — it transitions to Failed with
  `AssertionError`. Asserts express "this was expected to run, and a
  precondition is missing."

Conditions are checked first; only if all pass are asserts checked. All
entries are AND-ed. The check types:

| Type | Form | Passes when |
|---|---|---|
| `path` | `path:<path>` | the path exists (any type) |
| `file` | `file:<path>` | a regular file exists |
| `directory` | `directory:<path>` | a directory exists |
| `registry` | `registry:<key>` | the registry key exists |

Two constraints follow from peinit being single-threaded PID 1, which [must
never block](~peios/services-and-jobs/overview):

- **`registry:` checks are cache-only.** Accepted paths are exactly
  `Machine\System\Services`, `Machine\System\Init`, and
  `Machine\System\Services\<name>` with one nonempty service-name component.
  The roots resolve as present; the last form checks whether the service
  exists in the in-memory model. Init descendants, deeper Services paths,
  trailing separators, and other keys are load-time decode errors. No live
  registry read is made.
- **Filesystem checks run off the main loop.** `path:` /`file:`/`directory:`
  checks are performed by a short-lived forked helper, not a `stat()` on the
  event loop, because `stat()` can wedge on a hung mount.

That timeout is the `PreStartCheckTimeout` field, which defaults to **5
seconds**. A filesystem check that does not finish within it — say a `stat()`
wedged on a hung mount — is treated as **not satisfied**, fail-safe: peinit
kills the helper and treats the check as unmet, so a Condition skips the
service and an Assert fails it, rather than letting the event loop hang.

## The fd store

The fd store lets a service preserve file descriptors **across restarts** —
most usefully a listening socket, so a stateful daemon can restart without
dropping connections. It is opt-in: `FdStoreMax` (default 0) sets the maximum
number of fds peinit will hold; 0 disables it.

The mechanics ride on [sd_notify](~peios/services-and-jobs/service-types):

- A service stores an fd by sending `FDSTORE=1` with the fd attached (and
  optionally `FDNAME=<name>`; the default name is `stored`). peinit holds
  it.
- It removes one with `FDSTOREREMOVE=1` and `FDNAME=<name>`.
- On the **next start**, peinit injects the stored fds starting at fd 3, sets
  `LISTEN_FDS` to the count and `LISTEN_FDNAMES` to the colon-separated names,
  then clears the store. (This is the same `LISTEN_FDS` convention systemd
  uses, so software with existing fd-passing support works unmodified.)

The store **survives an automatic restart** (crash → restart policy → new
start) — that is the whole point. It is **cleared** on an *explicit* stop or
shutdown (the service is not coming back), and when a [removed
definition](~peios/services-and-jobs/defining-a-service) is finally discarded.

## A clean context

A service receives its standard streams and explicitly supplied descriptors.
It does not inherit peinit's privileged connections or signal handling. The
[launch-isolation
reference](~peios/reference/peinit/introduction/service-manager-boundaries#a-clean-context)
describes the descriptor and signal setup.

## The cgroup tree

peinit tracks a service's main process, hooks, and health checks in separate
cgroups beneath `/sys/fs/cgroup/peinit/`. It uses those groups for tracking
and clean termination, not resource limits.

A leaked cgroup warning means a process survived cleanup, often because I/O is
stuck. A later start can use a fresh `<service>.gen<N>` tree; that does not
repair or kill the stuck old process. Investigate the [Abandoned
state](~peios/services-and-jobs/the-service-lifecycle#abandoned).
The [cgroup
reference](~peios/reference/peinit/introduction/service-manager-boundaries#the-cgroup-tree)
retains the layout and rationale.

## Where to start

For the identity half of the launch — how the token is chosen and installed —
read [Service identity and
privileges](~peios/services-and-jobs/identity-and-privileges).

For what happens to the stdout/stderr pipes peinit holds, read [Service output
and logging](~peios/services-and-jobs/output-and-logging).

For how conditions, asserts, and hooks fit into the start sequence and its
states, read [The service
lifecycle](~peios/services-and-jobs/the-service-lifecycle).

### System-console aliases and logout

Peinit pins `TTYPath=/dev/console` to the kernel-selected terminal when
reading
service definitions. A display's moving `tty0` alias becomes `tty1`, so
switching
the foreground console does not move the session. Both arbitration and launch
use this fixed endpoint. When a terminal service's main process exits, peinit
kills its remaining session processes before handing the terminal on. A clean
exit under `RestartPolicy=Always` restarts the prompt at the base delay
without
spending the crash budget.
