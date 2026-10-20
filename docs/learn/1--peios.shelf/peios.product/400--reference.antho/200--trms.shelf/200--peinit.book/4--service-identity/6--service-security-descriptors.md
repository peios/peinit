---
title: Service Security Descriptors
description: A service carries two independent descriptors answering different questions — the rights, the default, the check, and hot reload.
---

A service carries two independent descriptors, and they answer different
questions.

**The registry key descriptor** on `Machine\System\Services\<name>`
controls who can read and write the service's *definition*. It is
enforced by LCS at key-open time and is not peinit's concern — peinit
reads definitions as SYSTEM.

**The ServiceSecurity descriptor** controls who can perform *runtime
operations* on the service through the control interface. It is stored
as a binary `ServiceSecurity` value on the same registry key, but
enforced by peinit rather than by LCS.
[*svcsd.servicesecurity-governs-runtime-operations]

The two are genuinely independent. An administrator might be able to
query a service's status without being able to read its configuration,
or the reverse. Runtime control and configuration access are separate
concerns and there is no reason for one to imply the other.

## Access rights

| Right | Bit | Grants |
|---|---|---|
| `SERVICE_QUERY_STATUS` | 0x0001 | Query state, PID, cause, health, warnings. [*svcsd.query-status-grants-status] |
| `SERVICE_START` | 0x0002 | Start the service. [*svcsd.start-grants-start] |
| `SERVICE_STOP` | 0x0004 | Stop the service. [*svcsd.stop-grants-stop] |
| `SERVICE_INTERROGATE` | 0x0008 | Reload the service. [*svcsd.interrogate-grants-reload] |
| `SERVICE_ALL_ACCESS` | 0x000F | The union of the four. |

Restart requires `SERVICE_START` and `SERVICE_STOP` together.
[*svcsd.restart-requires-start-and-stop] Reset
requires `SERVICE_STOP`, because clearing a Failed or Abandoned state is
the tail of stopping something rather than the head of starting it.
[*svcsd.reset-requires-stop]

The generic mapping peinit passes to AccessCheck:

| Generic right | Maps to |
|---|---|
| `GENERIC_READ` | `SERVICE_QUERY_STATUS` [*svcsd.generic-read-is-query-status] |
| `GENERIC_WRITE` | `SERVICE_START` \| `SERVICE_STOP` \| `SERVICE_INTERROGATE` [*svcsd.generic-write-is-start-stop-interrogate] |
| `GENERIC_EXECUTE` | `SERVICE_START` \| `SERVICE_STOP` \| `SERVICE_INTERROGATE` [*svcsd.generic-execute-is-start-stop-interrogate] |
| `GENERIC_ALL` | `SERVICE_ALL_ACCESS` [*svcsd.generic-all-is-service-all-access] |

<!-- lint-disable-next-line line-length -->
## Inheritance and the default [*svcsd.a-definition-with-no-value-takes-the-services-keys]

A service whose definition carries no `ServiceSecurity` value takes the
one on `Machine\System\Services` itself. The lookup is a single step to
that key, not a walk up the hierarchy, which is exact for the flat
layout definitions actually use. That includes the compiled-in registryd,
which takes it once Phase 2 has read the key and on every reload after;
a registry definition of registryd (§2.3) can give it a descriptor of its
own like any other service.

If that key has no `ServiceSecurity` either, peinit applies a built-in
default:
[*svcsd.the-built-in-default-grants-system-and-administrators-everything]

```text
O:SY G:BA D:(A;;0x000F;;;SY)(A;;0x000F;;;BA)(A;;0x0001;;;AU) S:(AU;FA;0x000F;;;WD)
```

SYSTEM and Administrators both get `SERVICE_ALL_ACCESS`. The SACL audits
every refusal, for everyone, which is what records a denial (below). An
administrator who can stop any service and shut the machine down gains
nothing by being unable to start one, so the default does not attempt a
narrower grant; a service that wants one carries its own
`ServiceSecurity` value.

Every authenticated principal gets `SERVICE_QUERY_STATUS`: what a service
is doing is not, by default, a secret from the people using the machine,
and a service whose state is a secret carries a `ServiceSecurity` that says
so.
[*svcsd.the-built-in-default-lets-everyone-query]

## The check

When a control command arrives, peinit:

1. Takes the caller's token, captured when the connection was accepted.
2. Resolves the target service and its ServiceSecurity descriptor. A
   command naming no definition and no addressable definition-removed
   entry returns `UNKNOWN_SERVICE`; peinit does not invent a descriptor
   to check against. [*svcsd.an-unknown-service-is-not-access-checked]
3. Calls AccessCheck with the caller's token, that descriptor, the
   generic mapping above, the right the command needs, and an audit
   context naming the service, `{kind: "service", service: {name}}`.
4. On denial, returns `ACCESS_DENIED`. The attempt is recorded by KACS,
   not by peinit: when the descriptor's SACL asks for it — and the
   built-in default's does — the check writes a
   `kacs.audit.access.checked` record with `object.kind` `service`,
   `object.service.name`, the caller's token, and the access requested
   and granted (§8.4).
   [*svcsd.a-denial-is-recorded-by-kacs-under-the-descriptors-sacl]
5. On grant, proceeds.

## The SACL is the audit policy, and who can drop it

A service's SACL decides which of its decisions are recorded. A
descriptor written to the registry is used as it is: one with no SACL
records nothing, and one that narrows the default's audits less.

`ServiceSecurity` is a whole registry value. Whoever can write it — on
the service's key, or on `Machine\System\Services`, whose value every
service without its own takes — can replace the descriptor, SACL
included, without holding `SeSecurityPrivilege`, which is what changing
an object's SACL otherwise needs. So those keys must be writable only by
principals trusted to change audit policy. By default they are: both
inherit the Machine hive root, which grants write to SYSTEM and
Administrators alone, and Administrators hold `SeSecurityPrivilege`.
Delegate a service through its `ServiceSecurity` DACL, never by
loosening the key's own descriptor; and a tool that changes only the
DACL must write the SACL back unchanged. The same holds for the control
descriptor in `Machine\System\Init` (§10.2).

## Hot reload [*svcsd.a-descriptor-change-needs-no-restart]

`ServiceSecurity` changes take effect on the next control request, with
no restart. A registry change notification triggers a configuration
reload, and the reload re-reads every descriptor. There is no cached
decision to invalidate — the check runs against the current descriptor
every time.

## Filtering, not denying [*svcsd.list-omits-rather-than-denies]

`list` returns only the services the caller has `SERVICE_QUERY_STATUS`
on. Services the caller cannot query are **omitted**, not denied: a
caller with no query rights anywhere receives an empty list and a
successful response. The denials are not surfaced to the caller; each
service's SACL decides whether KACS records them.

What the filtering protects is a service's *state*, which is what
`SERVICE_QUERY_STATUS` grants. That a service exists is not protected:
a command naming one is answered `UNKNOWN_SERVICE` or `ACCESS_DENIED`
according to whether it exists, and the definitions under
`Machine\System\Services` are readable. A client that can read the
definitions can therefore show a service `list` left out as one whose
state the caller can not see.
