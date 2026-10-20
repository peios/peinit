---
title: Who Can Manage a Service
type: concept
description: Check runtime permissions, grant service control without granting definition writes, and diagnose access denials.
related:
  - peios/services-and-jobs/identity-and-privileges
  - peios/services-and-jobs/controlling-services
  - kernel/internals/kacs/access-check/the-dacl-walk
---

To let someone start or stop a service, change its **ServiceSecurity**
permissions. Do not give them write access to the service definition to
grant control: definition writes can change the program and the identity it
runs as.

In **Services Manager**, select the service and inspect the commands you can
use. **Who can control it…** edits runtime control; **Who can change its
definition…** edits the registry key's permissions. If a command returns
`ACCESS_DENIED`, check its required right and the caller's identity before
widening either descriptor.

## Two descriptors, two questions

A service is associated with **two** descriptors that are easy to conflate but
answer different questions and are enforced by different components:

| Descriptor | Question | Stored | Enforced by |
|---|---|---|---|
| **Registry key SD** | Who can *read or edit the definition*? | On the `Machine\System\Services\<name>` key | LCS, at key-open time |
| **ServiceSecurity SD** | Who can *manage the running service*? | As the `ServiceSecurity` binary value on that key | **peinit**, on every control command |

The registry key SD is ordinary registry access
control and not peinit's concern —
peinit reads definitions as SYSTEM, which has full access. The
**ServiceSecurity** SD is peinit's domain, and the rest of this page is about
it.

They are genuinely independent. An administrator might be able to *query a
service's status* (ServiceSecurity grants `SERVICE_QUERY_STATUS`) but not
*read its configuration* (the registry key SD denies read) — or the reverse.
Runtime control and configuration access are separate concerns, and both
combinations are valid.

## Changing them from the desktop

On a GXWI desktop, select the service in **Services Manager**. Below what you
can do with it, the details pane says where its ServiceSecurity comes from:
set for this service, the default set for every service, or peinit's built-in
default. Two buttons open the permissions editor, and both are also on each
row's right-select menu.

- **Who can control it…** opens the ServiceSecurity descriptor. Its boxes are
  the service rights in words: **Full control**, **Start**, **Stop**,
  **Reload** (`SERVICE_INTERROGATE`) and **See its state**
  (`SERVICE_QUERY_STATUS`). Restart needs Start and Stop together, and Reset
  needs Stop. When you apply, Services Manager writes the result as the
  service's **own** `ServiceSecurity` value. From then on, a change to the
  default does not reach that service. peinit applies the new descriptor on
  the next command, and the window's buttons change to match.
- **Use the default** appears beside it once the service has its own value.
  After you confirm, it deletes that value, so the service takes the default
  again.
- **Who can change its definition…** opens the descriptor of the service's
  registry key. Its boxes are **Full control**, **Read**, and **Write** on the
  key. Entries you add there apply to the key and to any keys under it.

Under **Every service**, the pane says what a service without its own value
takes:

- **Default permissions…** opens the `ServiceSecurity` on
  `Machine\System\Services`. If there isn't one, it starts from peinit's
  built-in default. Applying writes it to that key, and it reaches every
  service without its own value at the next command.
- **Use the built-in default** appears once that key has a value. After you
  confirm, it deletes the value, so services without their own go back to
  peinit's built-in default.

Each of these is offered for changing only if you are allowed to change it:

- ServiceSecurity is a value on a registry key: the service's key, or
  `Machine\System\Services` for the default. Changing it, or deleting it,
  needs the right to set values on that key (`KEY_SET_VALUE`).
- The service key's own descriptor needs `WRITE_DAC`, and `WRITE_OWNER` for
  its owner.

If you are not allowed, the editor still shows who can do what and says why it
can't be changed, and the two "use the default" buttons are unavailable with
the reason. Services Manager finds this out by asking the registry for those
rights, not from your group memberships.

The same can be done from a terminal with `reg`. For example, this makes sshd
take the default again:

```sh
reg del 'Machine\System\Services\sshd' ServiceSecurity
```

## Service access rights

The ServiceSecurity descriptor grants these rights:

| Right | Bit | Grants |
|---|---|---|
| `SERVICE_QUERY_STATUS` | 0x0001 | Query state, PID, cause, health, warnings. |
| `SERVICE_START` | 0x0002 | Start the service. |
| `SERVICE_STOP` | 0x0004 | Stop the service. |
| `SERVICE_INTERROGATE` | 0x0008 | Reload the service. |
| `SERVICE_ALL_ACCESS` | 0x000F | All of the above — the "full access" granted to SYSTEM by default. |

`restart` requires **both** `SERVICE_STOP` and `SERVICE_START`, since it is a
stop followed by a start. `reset` requires `SERVICE_STOP`.

When peinit evaluates the descriptor it maps the generic rights as follows, so
a descriptor written with generic rights behaves sensibly:

| Generic | Maps to |
|---|---|
| `GENERIC_READ` | `SERVICE_QUERY_STATUS` |
| `GENERIC_WRITE` | `SERVICE_START` \| `SERVICE_STOP` \| `SERVICE_INTERROGATE` |
| `GENERIC_EXECUTE` | `SERVICE_START` \| `SERVICE_STOP` \| `SERVICE_INTERROGATE` |
| `GENERIC_ALL` | `SERVICE_ALL_ACCESS` |

## The default descriptor

If a service has no `ServiceSecurity` value, it **inherits** the one on
`Machine\System\Services` itself. If that key has none either, peinit applies
a built-in default:

- **SYSTEM** (`S-1-5-18`) — full access.
- **Administrators** (`S-1-5-32-544`) — full access.
- **Authenticated Users** (`S-1-5-11`) — query only.
- **Everyone** (`S-1-1-0`) — every refusal audited, in its SACL.

So out of the box, administrators can do anything with a service, everyone who
is signed in can see what it is doing, and every refused command is recorded.
To let someone else start or stop a particular service, give that service a
`ServiceSecurity` that grants it; anyone signed in can reach the control
socket, so a grant to them takes effect.

> [!WARNING]
> A `ServiceSecurity` value carries the service's audit policy as well as its
> permissions, and it is written whole. Anyone who can write it — that is,
> anyone who can write the service's registry key, or
> `Machine\System\Services` — can remove its SACL and so stop its denials
> being recorded. Keep those keys writable only by administrators, as they are
> by default. To let someone manage a service, grant them rights in its
> `ServiceSecurity`; do not give them write access to its key.

ServiceSecurity is **hot-reloaded**: a change to the value in the registry
takes effect on the **next control request**, with no service restart. peinit
picks the change up through a registry
notification. This is why ServiceSecurity
is in its own [mutability class](~peios/services-and-jobs/defining-a-service)
— access policy should be able to change without disturbing a running service.

## How a command is authorized

Every control command runs the same gate:

1. peinit captures the caller's
   [token](~peios/services-and-jobs/identity-and-privileges) from the kernel
   (the `KACS_SO_PEER_TOKEN` socket option) — the caller's *effective*
   identity at connection time, so if the caller is
   [impersonating](~kernel/internals/kacs/impersonation/overview), the impersonated identity
   is what is checked.
2. peinit resolves the target service and its ServiceSecurity descriptor.
3. peinit runs [AccessCheck](~kernel/internals/kacs/access-check/the-dacl-walk): the caller's
   token against the descriptor, for the right the command needs.
4. **Denied** → return `ACCESS_DENIED`. The kernel records the attempt —
   caller, target service, rights requested, and granted — as a
   `kacs.audit.access.checked` event, because the descriptor's SACL asks it
   to.
5. **Granted** → execute the command.

> [!NOTE]
> Every denial is recorded by default: the built-in descriptor's SACL audits
> every refusal, for everyone, and peinit names the service in each check so
> the record says which one it was. Look for it in the event viewer as
> `kacs.audit.access.checked` with **object kind** `service` and the service's
> name. A `ServiceSecurity` you write yourself is used as it is, so keep a
> SACL on it (for example `S:(AU;FA;0xF;;;WD)`) if you still want its denials
> recorded. If you are debugging a denial, the audit record has everything you
> need; see Debugging a denial.

## The system control descriptor

Some operations are not about any one service — `shutdown` and `reload-config`
act on the whole system, and `boot` reports on it. These are checked against
**peinit's own** descriptor, stored at `Machine\System\Init\ControlSecurity`:

| Right | Bit | Grants |
|---|---|---|
| `SYSTEM_SHUTDOWN` | 0x0001 | Initiate poweroff, reboot, or halt. |
| `SYSTEM_RELOAD_CONFIG` | 0x0002 | Re-read all definitions and rebuild the graph. |
| `SYSTEM_QUERY_STATUS` | 0x0004 | Ask how this boot went (`svctl boot`). |

| Generic | Maps to |
|---|---|
| `GENERIC_READ` | `SYSTEM_QUERY_STATUS` |
| `GENERIC_WRITE` | `SYSTEM_RELOAD_CONFIG` |
| `GENERIC_EXECUTE` | `SYSTEM_SHUTDOWN` |
| `GENERIC_ALL` | all three |

The default grants **SYSTEM** and **Administrators** all three rights, and
**Authenticated Users** `SYSTEM_QUERY_STATUS`, as the default service
descriptor lets everyone signed in query a service. peinit loads this
descriptor at boot and hot-reloads it on registry change, exactly like
ServiceSecurity. A descriptor written before `SYSTEM_QUERY_STATUS` existed
grants it only through `GENERIC_READ`, `GENERIC_ALL` or the bit itself.

## Jobs have descriptors too

A [submitted job](~peios/services-and-jobs/jobs-and-operations) is a securable
object in the same way. It carries its own descriptor from the moment it
exists — by default owned by the submitter and granting `JOB_QUERY`,
`JOB_STOP`, and `JOB_SIGNAL` to the submitter, SYSTEM, and Administrators,
and *nothing* to the identity the job runs as — and every job command, on
either socket, is checked against it. The one thing no descriptor inside
peinit governs is who can *submit*: that is the file descriptor on the jobs
socket, checked by the kernel when a process connects.

## The list command filters, it does not deny

`list` is access-control-aware in a quieter way: it returns only the services
the caller has `SERVICE_QUERY_STATUS` on, and **omits** the rest. A
caller with no query rights gets an empty list, not a denial. What this
protects is a service's *state*; that a service exists is not a secret. A
command naming a service is answered `UNKNOWN_SERVICE` only if it does not
exist, and the definitions under `Machine\System\Services` are readable.

## The boundaries that hold

A few invariants are worth stating outright, because they are what make this
trustworthy:

- **peinit never bypasses AccessCheck for a control operation.** No backdoor,
  no override flag, no "trust localhost."
- **The descriptors are the only policy inputs.** peinit consults the
  ServiceSecurity and ControlSecurity descriptors and nothing else — not
  config files, not environment variables, not hardcoded lists.
- **One service's state is never exposed to another without a check.**
  `status` is per-service access-controlled; `list` filters. The same holds
  for jobs: `job status` is checked, `job list` filters.

## Where to start

For the *other* half of the security model — what a service can reach once it
is running — read [Service identity and
privileges](~peios/services-and-jobs/identity-and-privileges).

For the commands these rights gate, read [Controlling
services](~peios/services-and-jobs/controlling-services).

For the descriptor and AccessCheck machinery itself, read Security
descriptors and [Access
decisions](~kernel/internals/kacs/access-check/the-dacl-walk).
