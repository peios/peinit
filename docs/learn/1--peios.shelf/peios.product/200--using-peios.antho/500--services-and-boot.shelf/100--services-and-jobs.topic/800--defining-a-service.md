---
title: Defining a Service
type: how-to
description: Inspect, validate, create or edit a service definition, then verify when its changes take effect.
related:
  - peios/services-and-jobs/overview
  - peios/services-and-jobs/service-types
  - peios/services-and-jobs/controlling-services
  - peios/services-and-jobs/registry-key-reference
---

Use **svctl definition** or **Services Manager** to change a service's
configuration. Both check field values before writing them. You need
permission to change the service's registry key; permission to start or stop
the service is separate.

Start by reading the current definition and runtime state:

```sh
svctl definition show sshd
svctl status sshd
```

A definition lives at `Machine\System\Services\<name>`. Its typed registry
values describe the program, identity, dependencies, triggers, and supervision
policy. The name of the key is the name you use in `svctl`.

## Change and verify

1. Read the current definition. Decide which field needs to change and [when
   it will apply](#field-mutability-when-a-change-takes-effect).
2. Use `svctl definition edit` or **Edit definition…** in Services Manager.
   Check the reported changes before restarting a running service.
3. Validate the saved definition with `svctl definition validate <service>`.
   This checks fields; identity, privilege names, and dependency targets also
   need their runtime checks.
4. If launch settings require it, [restart the
   service](~peios/services-and-jobs/controlling-services) during an
   appropriate interruption window. `svctl reload-config` re-reads definitions
   but does not replace a running process.
5. Run `svctl status <service>` again. If it fails, inspect the cause and
   [logs](~peios/services-and-jobs/output-and-logging).

> [!WARNING]
> A definition controls what runs and under which identity. Keep
> definition-write access limited to trusted administrators. Grant runtime
> start/stop rights through
> [ServiceSecurity](~peios/services-and-jobs/who-can-manage-a-service), rather
> than giving an operator permission to rewrite the program or its identity.

## Creating and changing a definition

A definition is a registry key, so `reg` can write one. Two tools also know
what each field means, and check a definition before writing it.

### From a terminal: `svctl definition`

```sh
svctl definition show sshd
svctl definition validate sshd
svctl definition create web --set ImagePath=/usr/bin/web --set Requires=lpsd --set Requires=netd:routed
svctl definition edit web --set StopTimeout=20 --unset Wants
```

`def` is short for `definition`.

- **Fields go by their registry names**, in any case.
- **Values are given as text:**
  - a number as digits;
  - `yes` or `no`;
  - a choice by its name (`Type=Oneshot`);
  - a list with one `--set` for each item, in order.
- **A change is checked first, as peinit checks it.** It goes through peinit's
  own decoder, and every `timer:` schedule is parsed. If peinit would reject
  it, nothing is written and you are told why.
- **Only the values that change are written,** in one registry transaction. If
  one of them has changed since svctl read it, nothing is written.
- **Values that are not fields are kept** as they are.
- **`edit` names any changed fields that wait for a restart** on a running
  service. See [the mutability
  classes](#field-mutability-when-a-change-takes-effect).

Some things can't be checked until later:

- **privilege names**, when the token is made;
- **whether the services a definition names exist**, when the graph is built;
- **the identity**, by authd.

`svctl status` after the change is the final word.

### Manager

In [Services
Manager](~peios/services-and-jobs/controlling-services#manager),
**Edit definition…** opens the selected service's definition in a window of
its own. It's in the details pane and on each row's right-select menu. It
reads **Definition…** when you can only read the definition. **New service…**
on the bar defines a new one. A service's definition opens in one window at a
time: while it is open, **Edit definition…** is unavailable for that service.

- **Every field is shown, by group.** Each has its name in words with the
  registry name beside it, its default, and when a change takes effect.
- **What you type is checked when you leave the field,** as peinit checks it.
  Anything wrong is said beside the field, and **Save** stays unavailable
  until it is put right.
- **Each timer's schedule says when it next comes round,** under **Triggers**,
  in local time. A schedule that never comes round, such as `*-02-30`, is
  marked as one that never starts the service. peinit takes such a definition
  and arms its other timers, so it does not stop you saving. These times are
  the schedule's, before any jitter. When the timer will really fire is in the
  details pane, once it is saved (see [Controlling
services](~peios/services-and-jobs/controlling-services#manager)).
- **Save writes as svctl does,** from the button or with **Ctrl+S**. Only the
  changes are written, in one transaction, and it is refused if someone else
  changed a value meanwhile. **Revert** reads the definition again. If the
  service is running, Save says which changes wait for a restart.
- **Delete…** asks first. A running service carries on until it stops.
- **What you can do is asked of the registry:**
  - changing needs the right to set values on the service's key;
  - defining a service needs the right to create keys under
    `Machine\System\Services`;
  - deleting needs `DELETE` on the key.

  Without those rights, the definition is shown with every field fixed and the
  reason said.
- **Who can control the service** (`ServiceSecurity`) is changed with **Who
  can control it…**, not here. See [Who can manage a
  service](~peios/services-and-jobs/who-can-manage-a-service).

## Field mutability: when a change takes effect

Every field falls into one of four mutability classes. This table is the one
to keep handy.

| Class | When a change takes effect | Fields |
|---|---|---|
| **Immutable at runtime** | Next **restart** only. | `ImagePath`, `Type`, `Identity`, `RequiredPrivileges`, `ErrorControl`, `RemainAfterExit`, and, while it is running, `Triggers` and `Disabled` |
| **Apply on next start** | Next **start** or explicit graph reload — not while running. | `Requires`, `Wants`, `BindsTo`, `Conflicts`, `OnFailure`, `Conditions`, `Asserts` |
| **Hot-reloaded** | Next relevant **event**, no restart. | `ServiceSecurity` (next control request) |
| **Reloadable at runtime** | Next relevant **operation** (start, health-check cycle, …). Launch settings apply to the next process, not the current one. | `Arguments`, `SuccessExitCodes`, all timeout/retry values, the health-check fields, `RestartPolicy`, `Environment`, `WorkingDirectory`, the hooks and `HookIdentity`, `Readiness`, `NotifyAccess`, `LimitNOFILE`/`LimitCORE`, `FdStoreMax`, `TTYPath`/`TTYPrecedence`, `RuntimeDirectories`, `Provides`, `TimerPersistent`/`TimerJitter`, `SafeMode`, `DisplayName`, `Description` |

On a service that is not running, `Triggers` and `Disabled` take effect as
soon as peinit has read the change: that is what arms a timer added to an
inactive service.

The practical reading: changing *what a process is or runs as* (`ImagePath`,
`Identity`, `Type`, privileges, `ErrorControl`) is fundamental enough that
it only applies when a fresh process starts — you must `restart`. Changing
*policy that peinit consults each time it acts* (timeouts, restart behavior,
health checks) is picked up the next time peinit acts. And `ServiceSecurity`
is special: it is re-read on every control request, so an access-control
change takes effect on the very next command without touching the running
service.

`Environment`, `Arguments`, `WorkingDirectory`, and process limits do not
alter an already-running process. Even though peinit can reload those fields,
the process receives them only on its next start.

For inactive services the rules are simpler, because there is no running
process to protect: a new service entry becomes available once the change
notification is processed; a changed timer arms on the next evaluation; a
changed dependency takes effect on the next start.

> [!NOTE]
> `reload-config` is the explicit, atomic way to make peinit re-read
> *everything*. It builds and validates a complete new graph snapshot and
> swaps it in only if validation passes; running services are not disturbed.
> It is covered with the rest of the command set in [Controlling
> services](~peios/services-and-jobs/controlling-services).

## Removing a service definition

Use `svctl definition delete web` only when you intend to remove the
definition of `web`. To stop its current work, use `svctl stop web` instead.

Deleting a definition from the registry does **not** kill a running instance.
peinit learns of the removal through the same notification path and behaves
according to whether the service is running:

- **Not running** (Inactive, Failed, Completed, Skipped, Abandoned, Backoff):
  peinit discards the in-memory entry immediately. There is nothing to
  supervise.
- **Running** (Active, Starting, Reloading, Stopping): the running process is
  a [job](~peios/services-and-jobs/jobs-and-operations), and a job outlives
  its definition. peinit marks the entry **definition-removed**, keeps the
  cached definition only to finish supervising the existing instance, and does
  *not* restart it when it exits. Once it exits, the entry — and any stored
  fds — are discarded.

A removal that leaves a dangling hard dependency does not apply: peinit
refuses the configuration read with `MissingHardDependency`. Remove or update
the dependent definitions first. A definition-removed entry therefore has no
dependents.

For a definition-removed entry, `stop` still works so you can drain it
cleanly, but `start`, `restart`, and `reload` are rejected with
`UNKNOWN_SERVICE` — there is no definition to start from. A `status` query
reports it with a `definition_removed: true` flag so the draining instance is
never invisible.

> [!IMPORTANT]
> Removing a definition is not how you stop a service. It stops *future
> management*, not work in progress. To actually stop a running service,
> `stop` it. To prevent it from auto-starting, set `Disabled=1` (see [Triggers
> and timers](~peios/services-and-jobs/triggers-and-timers)). To prevent it
> from being started at all, deny `SERVICE_START` in its
> [ServiceSecurity](~peios/services-and-jobs/who-can-manage-a-service)
> descriptor.

## Service names

A service name must be made of characters from `[A-Za-z0-9._-]` and be 1–128
bytes long. Anything else is a validation error. Two characters are pointedly
excluded:

- **`/`** — names map directly onto cgroup ids and registry key names, and a
  slash would be ambiguous in both.
- **`:`** — reserved for peinit-internal synthetic names (for example, the way
  a hook job is labeled).

The name is how you refer to the service everywhere: in `svctl` commands, in
another service's dependency list, in a
[ServiceSecurity](~peios/services-and-jobs/who-can-manage-a-service)
descriptor, and in the [per-service
SID](~peios/services-and-jobs/identity-and-privileges) derived from it.

## The definition schema

A definition is a set of typed registry values. Rather than list all of them
in one wall of rows, the tables below group the fields by what they are *for*,
with a pointer to the page that explains each group in depth. Every field is
optional unless noted; the only hard requirement is `ImagePath`. The full
type-and-default catalog lives in the [Registry key
reference](~peios/services-and-jobs/registry-key-reference).

**What to run** — the binary and its execution context.

| Field | Default | Purpose |
|---|---|---|
| `ImagePath` *(required)* | — | Absolute path to the service binary. |
| `Arguments` | — | Argument list passed to the binary. |
| `WorkingDirectory` | `/` | Working directory for the process. |
| `Environment` | — | `KEY=VALUE` pairs added to the environment. |
| `RuntimeDirectories` | — | Private directories created under `/run` immediately before the process starts. See [The execution environment](~peios/services-and-jobs/execution-environment). |
| `LimitNOFILE`, `LimitCORE` | — | `RLIMIT_NOFILE` / `RLIMIT_CORE`. |

**What kind of service** — type and readiness. See [Simple and Oneshot
services](~peios/services-and-jobs/service-types).

| Field | Default | Purpose |
|---|---|---|
| `Type` | Simple | `Simple` (long-running) or `Oneshot` (run-to-completion). |
| `Readiness` | Notify | `Notify` (`READY=1` through sd_notify) or `Alive` (ready when the process exists). Ignored for Oneshot. |
| `RemainAfterExit` | 0 | Oneshot only — stay in `Completed` after a successful exit. |
| `SuccessExitCodes` | — | Non-zero exit codes to treat as success. |

**When to start** — triggers and conditions. See [Triggers and
timers](~peios/services-and-jobs/triggers-and-timers).

| Field | Default | Purpose |
|---|---|---|
| `Triggers` | — | `boot`, `boot:settled`, `timer:<schedule>`, `tty:released`. Absent = demand-only. |
| `Disabled` | 0 | If 1, triggers must not activate the service (manual start still allowed). |
| `SafeMode` | 0 | Eligible within the boot-triggered set in [Safe mode](~peios/boot/boot-and-boot-modes); does not add a boot trigger. |
| `Conditions`, `Asserts` | — | Start-time checks. A failed condition *skips*; a failed assert *fails*. |

**Who it runs as** — identity and privileges. See [Service identity and
privileges](~peios/services-and-jobs/identity-and-privileges).

| Field | Default | Purpose |
|---|---|---|
| `Identity` | `LocalService` | Principal name, SID, or `Service` for a distinct virtual service account. |
| `RequiredPrivileges` | — | Privilege allow-list; everything else is stripped from the token. |
| `HookIdentity` | service's `Identity` | Identity for `ExecStartPre`/`ExecStartPost` hooks. |

**How it relates to other services** — dependencies. See [Dependencies and
ordering](~peios/services-and-jobs/dependencies).

| Field | Purpose |
|---|---|
| `Requires` | Hard dependencies — must be satisfied first; their failure fails this service. |
| `Wants` | Soft dependencies — attempted first if present and enabled, but their failure does not block this service. |
| `BindsTo` | Runtime coupling — if the target stops, this stops too. |
| `Conflicts` | Mutual exclusion — starting this stops the named services. |
| `OnFailure` | Service to start when this one enters `Failed`. |

**How it is kept alive** — supervision and health. See [Keeping services
running](~peios/services-and-jobs/supervision).

| Field | Default | Purpose |
|---|---|---|
| `ErrorControl` | Normal | `Normal` (stay Failed) or `Critical` (sync + reboot on irrecoverable failure). |
| `RestartPolicy` | OnFailure | `Never` / `OnFailure` / `Always`. |
| `RestartMaxRetries`, `RestartWindow`, `RestartDelay` | 5 / 120 / 1 | Restart budget, the window of health that resets it, and the backoff base. |
| `HealthCheck`, `HealthCheckInterval`, `HealthCheckTimeout`, `HealthCheckRetries` | — / 30 / 5 / 3 | Active health-check command and its timing. |
| `WatchdogTimeout` | 0 | Expected interval between `WATCHDOG=1` pings; 0 disables. |

**The transition phases** — hooks and timeouts. See [The execution
environment](~peios/services-and-jobs/execution-environment) and [The service
lifecycle](~peios/services-and-jobs/the-service-lifecycle).

| Field | Default | Purpose |
|---|---|---|
| `ExecStartPre`, `ExecStartPost` | — | Commands run before the binary / after readiness. |
| `ExecReload` | (SIGHUP) | Reload command or `signal:<NAME>`. |
| `StartTimeout`, `StopTimeout` | 30 / 10 | Seconds for the whole start sequence / between SIGTERM and SIGKILL. |

**The remaining knobs** — scheduling, notify, fds, metadata.

| Field | Default | Purpose |
|---|---|---|
| `TimerPersistent`, `TimerJitter` | 1 / 0 | Catch up missed timer runs after reboot / random delay per firing. |
| `NotifyAccess` | Main | Who can send sd_notify messages (only `Main` is supported). |
| `FdStoreMax` | 0 | Size of the per-service fd store; 0 disables it. |
| `ServiceSecurity` | inherit | Security descriptor controlling who can manage the service. |
| `DisplayName`, `Description` | — | Human-readable labels for status output. |

### Forward compatibility

The schema version lives at `Machine\System\Services\SchemaVersion` (
`1`). peinit is deliberately forward-compatible:

- **Unknown values are ignored.** A definition written for a newer peinit does
  not break an older one.
- **A newer schema version does not block boot.** peinit logs a warning and
  continues.
- **The schema only grows.** New capability arrives as new optional fields,
  never as a breaking change to an existing one.

One defensive rule cuts the other way: a *known* field must not appear more
than once in a collected definition. A duplicated known field is a validation
error, even though the registry would ordinarily give you at most one value
per name.

## peinit works from a snapshot, not the live registry

Here is the idea that explains most surprises. peinit does **not** re-read the
registry every time it touches a service. It reads definitions at well-defined
moments and operates on an in-memory model in between.

Two layers of snapshotting stack on top of each other:

- **Boot generation.** At the start of boot, peinit reads *all* definitions,
  builds the dependency graph, and validates it. The whole boot runs against
  that one snapshot. Changes made *during* boot — by an install script, a
  post-hook — do not perturb the boot in progress.
- **Activation generation.** When peinit starts a specific service, it
  snapshots *that* service's definition for the entire start. Pre-exec hooks,
  the token request, the readiness timeout, the first health checks — all use
  the values captured at activation. Edit a field while the service is
  `Starting`, and the edit waits for the next start.

peinit learns about registry edits through change
notifications: it subscribes to
`Machine\System\Services\` and `Machine\System\Init\` at boot, and processes
events in its event loop at a time of its choosing. If the notification queue
overflows — a bulk admin operation, a flurry of scripted writes — peinit
detects the overflow marker and does a full `reload-config` to resynchronise.
You never have to think about the queue; you do have to know that *a change is
picked up, not pushed*, and that when it takes *effect* depends on the field.

## Where to start

To understand the `Type` and `Readiness` fields and the Simple/Oneshot split,
read [Simple and Oneshot services](~peios/services-and-jobs/service-types).

To understand how a definition becomes a running, supervised process — and
what each state in a `status` output means — read [The service
lifecycle](~peios/services-and-jobs/the-service-lifecycle).

For the complete type-and-default catalog of every registry key peinit reads,
see the [Registry key
reference](~peios/services-and-jobs/registry-key-reference).
