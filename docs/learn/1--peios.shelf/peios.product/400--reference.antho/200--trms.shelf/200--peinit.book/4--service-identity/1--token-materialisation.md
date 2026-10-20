---
title: Token Materialisation
description: Every service process runs with a KACS token — where peinit obtains or creates it, and where it is installed.
---

Every service process runs with a KACS token that determines its
identity and its access rights. peinit obtains or creates that token and
installs it on the child before exec. It never shares its own token —
even an `Identity=SYSTEM` service receives a separately materialised
token of its own. [*identity.a-service-never-shares-peinits-token]

Which route the token comes from depends on the `Identity` field:

| Identity | Source | Mechanism |
|---|---|---|
| `SYSTEM` | Minted by peinit from its own identity | `kacs_create_token`. §4.2 |
| `Service` | authd | A virtual account unique to this service. §4.3 |
| `LocalService` / `NetworkService` | authd | A shared service account, with this service's SID as a group. §4.3 |
| Another principal | authd | A principal designated for service logon. §4.3 |
| Absent or empty | authd | Defaults to `LocalService`. [*identity.an-absent-identity-defaults-to-localservice] |

peinit reads its own token with `kacs_open_self_token` requesting the
real token rather than any impersonation, and opens it query-only: it is
a template to copy from, never a thing to hand out.

<!-- lint-disable-next-line line-length -->
## Where a token is materialised [*identity.materialisation-is-per-launched-process]

Materialisation happens at the point of use, per launched process, not
once per service. A service that runs a pre-exec hook, a main process,
and then a health check materialises three tokens.

| Context | Identity used |
|---|---|
| Main process | `Identity` |
| `ExecStartPre` / `ExecStartPost` | `HookIdentity` if set, otherwise `Identity` [*identity.hooks-use-hookidentity-when-set] |
| Health checks | `Identity`, always [*identity.health-checks-always-use-identity] |
| `ExecReload` external command | `Identity`, always [*identity.execreload-always-uses-identity] |
| Submitted jobs | The prepared primary token: the submitter's own, or the token the kernel attached to the submission (§8.5) |

Health checks and reload commands deliberately do not honor
`HookIdentity`. A health check reports on the service's own health and
should see what the service sees; a reload command acts on the running
service. `HookIdentity` exists for setup work — creating directories in
privileged locations, running a migration — which is a different job
from either.

> [!NOTE]
> `HookIdentity` typically grants *more* than the service itself.
> peinit does not validate filesystem permissions on hook binaries, so
> where it grants elevated privileges the administrator is responsible
> for the hook binary and its parent directories not being writable by
> anything lower-privileged.

If materialisation fails at any point — authd unreachable, an identity
that cannot be resolved, a KACS error — no child exists yet, and the
start fails with `ParentSetupFailure` for the main process or
`PreHookFailure` for a hook.
[*identity.a-materialisation-failure-fails-the-start]
