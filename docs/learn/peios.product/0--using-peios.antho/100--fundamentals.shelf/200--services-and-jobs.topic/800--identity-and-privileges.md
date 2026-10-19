---
title: Service Identity and Privileges
type: concept
description: Choose a service account, keep privileges minimal, protect elevated hooks, and distinguish process identity from management permissions.
related:
  - peios/services-and-jobs/defining-a-service
  - peios/services-and-jobs/who-can-manage-a-service
  - peios/services-and-jobs/execution-environment
  - kernel/internals/kacs/tokens/the-token-model
  - kernel/internals/kacs/privileges/the-privilege-model
  - peios/peinit/service-identity/the-authd-path
---

A service's **Identity** decides what its process can access. Its
**ServiceSecurity** descriptor decides who can start, stop, or query it; the
registry key descriptor decides who can edit the definition. Check the right
boundary before changing permissions.

Read the definition with `svctl definition show <service>`. Prefer the
identity and privileges the service actually needs. An absent or empty
`Identity` means `LocalService`; `SYSTEM` must be explicit. peinit gives each
service a separate token and never shares its own SYSTEM token.

## A distinct virtual service account

Set `Identity` to `Service` to run under the service's own `S-1-5-80-…` SID as
its user identity. authd derives it from the attested service name, using the
same
case-insensitive derivation as the existing service group. Only SYSTEM PID 1
can
request the attestation. No local principal record or stored credential is
created, and this does not provide an interactive logon path.

Unlike shared LocalService, two differently named virtual services have
distinct
file and process owners. Grant storage access and policy privileges to the
individual service SID. Renaming the service changes its identity and requires
coordinated updates to those grants. The default remains LocalService for
existing definitions; choosing Service is explicit.

The standard timed, resolvd and trustd deployments also use distinct Service
identities, retaining their existing service-SID grants. Their SYSTEM
preparation hooks, where present, remain separate from their main processes.

The standard eventd deployment uses this identity with
SeChangeNotifyPrivilege,
SeSecurityPrivilege (KMES consumption), and SeAuditPrivilege (event emission).
Its System integrity level allows reception of higher-integrity client tokens;
it does not grant SYSTEM account membership or SeTcbPrivilege. A short SYSTEM
pre-start hook initializes registry authorization defaults, then exits before
the long-running daemon starts. Private stores name eventd's own SID.

Services that need token minting/installation, raw network administration,
device management, or pre-authd bootstrap still require separate assessment.
Granting SeTcbPrivilege solely to make an identity change work defeats the
purpose of moving a service out of the machine-wide trust boundary.

The local principal source lpsd also runs as Service, with only
SeChangeNotifyPrivilege and System integrity for handling client tokens. Its
package provisions its private state scope; credential files are owned by lpsd
and admit only lpsd and SYSTEM. Its administrative API still admits SYSTEM and
Administrators. `/run/psi.sock` admits service connections, but authd
registers a
source only when its enabled service SID matches the configured source
allowlist.
Transport admission does not confer source authority.

## Privilege restriction

A token comes from its source (authd or the SYSTEM mint) with a default set of
[privileges](~kernel/internals/kacs/privileges/the-privilege-model). `RequiredPrivileges` lets a
definition trim that set down to only what the service needs:

- peinit reads the `RequiredPrivileges` allow-list and **removes every
  privilege not on it** from the token before exec.
- Restriction is **purely subtractive.** peinit removes privileges; it never
  adds them. A service cannot acquire a privilege its token's source did not
  grant.
- If `RequiredPrivileges` is absent, the token's default privilege set is used
  unchanged.

This is least-privilege made concrete: `eudev`, for instance, runs as SYSTEM
(it needs device access during early boot) but with its privilege set stripped
to the minimum it actually uses, so a compromise of `eudev` does not hand an
attacker the full SYSTEM privilege set. Removal is permanent for the life of
that token — it is not a disable that the service can re-enable. See
[Privileges](~kernel/internals/kacs/privileges/the-privilege-model) for what the individual privileges
grant.

## Which identity runs what

A service is not only its main process — it has hooks, health checks, and a
reload command, and each runs under a defined identity:

| Context | Runs as |
|---|---|
| **Main process** | The service's `Identity`. |
| **`ExecStartPre` / `ExecStartPost`** | `HookIdentity` if set, otherwise the service's `Identity`. |
| **Health checks** | The service's `Identity`, always. |
| **`ExecReload`** (command form) | The service's `Identity`, always — `HookIdentity` does not apply. |
| **[Submitted jobs](~peios/services-and-jobs/jobs-and-operations)** | The submitting process's own primary token — or, if the submitter attached one, the token the kernel verified it could convey (its client's identity, when impersonating). Never an identity named in the request. |

Token materialisation for hooks follows the same rules: a `HookIdentity` of
`SYSTEM` is minted; any other principal is requested from authd, at the point
the hook runs.

`HookIdentity` exists so hooks can run with *different* — usually higher —
privileges than the service itself: creating directories in privileged
locations, or running a database migration as an admin identity, before the
long-running daemon drops to a lesser identity.

> [!WARNING]
> peinit does **not** validate filesystem permissions on hook binaries. If
> `HookIdentity` grants elevated privileges, it is the administrator's
> responsibility to ensure the hook binary and every parent directory are not
> writable by a lower-privileged identity — otherwise a less-trusted principal
> could replace the hook and run code as the elevated identity.
> ([FACS](~kernel/internals/kacs/file-security/the-handle-model) enforces KACS descriptors on managed
> filesystems, but peinit itself performs no such check — protecting hook
> binaries still depends on correct SDs from packaging and controlled paths.)

## The per-service SID

Every service token carries a SID derived from its service name. Granting
access to that SID can distinguish two services even when they share
LocalService. With `Identity=Service`, it is also the token's user identity.

Renaming a service changes its SID. Update grants that name the old SID as
part of the same planned change. The [SID derivation
reference](~peios/advanced-peios/peinit/introduction/service-manager-boundaries#the-per-service-sid)
gives the algorithm.

## How a token is materialised

`Identity=SYSTEM` uses a separate token minted by peinit. Other identities use
authd; if authd cannot issue the required identity, the start fails rather
than falling back to SYSTEM.

### The SYSTEM path

Use SYSTEM only when the service requires that authority, such as the services
needed before authd is available. `registryd` and `authd` use this bootstrap
path. Standard eventd and lpsd deployments now use their own `Service`
identities and start through authd.

### The authd path

For a non-SYSTEM identity, check that authd is available and the account is
permitted for service logon. The `authd.service.attested` event records the
service, issued identity and privileges, or a refusal reason. peinit derives
an authority dependency for a non-SYSTEM service or hook identity.

The [token-materialisation
reference](~peios/advanced-peios/peinit/introduction/service-manager-boundaries#how-a-token-is-materialised)
retains the minting and authority flow. The current authd request is specified
in the [TRM authd
path](~peios/advanced-peios/peinit/service-identity/the-authd-path).

## Security invariants

The identity model rests on a few rules peinit never breaks:

1. **peinit never shares its SYSTEM token.** Even `Identity=SYSTEM` services
   get a separately minted token.
2. **peinit never drops its own SYSTEM identity.** PID 1 runs as SYSTEM for
   the life of the system — its identity is axiomatic, granted by the kernel
   at boot.
3. **Privileges are subtractive only.** `RequiredPrivileges` removes; it can
   never add.
4. **Identity is deterministic.** Every service runs as a known principal.
   `SYSTEM` must be declared explicitly; an empty `Identity` is the minimal
   `LocalService`, never SYSTEM.
5. **A submitted job never runs as an identity its submitter could not act
   as.** peinit installs only the submitter's own primary token or a token the
   kernel gated on the way in; it never opens a token by a PID or descriptor a
   submitter names, and there is no identity field to name one.

## Where to start

`Identity` decides what a service *can do*; the [ServiceSecurity
descriptor](~peios/services-and-jobs/who-can-manage-a-service) decides *who
can manage it* — two independent concerns. Read [Who can manage a
service](~peios/services-and-jobs/who-can-manage-a-service) for the other
half.

To see how the token is installed alongside the rest of the process setup,
read [The execution
environment](~peios/services-and-jobs/execution-environment).

For the identity primitives themselves — tokens, SIDs, privileges — start at
[Tokens](~kernel/internals/kacs/tokens/the-token-model).
