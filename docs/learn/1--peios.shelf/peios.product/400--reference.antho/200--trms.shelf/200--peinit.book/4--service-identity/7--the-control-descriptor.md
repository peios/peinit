---
title: The Control Descriptor
description: Shutdown, configuration reload,  and the boot query are checked against peinit's own descriptor rather than any service's.
---

Three commands are not about any one service: shutting the system down,
re-reading the configuration, and asking how the machine booted. They
are checked against peinit's own descriptor, stored at
`Machine\System\Init\ControlSecurity` as a binary value.
[*svcsd.control-operations-use-peinits-own-descriptor]

## Access rights

| Right | Bit | Grants |
|---|---|---|
| `SYSTEM_SHUTDOWN` | 0x0001 | Initiate poweroff, reboot,  or halt. [*svcsd.control-shutdown-grants-poweroff-reboot-halt] |
| `SYSTEM_RELOAD_CONFIG` | 0x0002 | Re-read all definitions from the registry. [*svcsd.control-reload-config-grants-a-definition-reread] |
| `SYSTEM_QUERY_STATUS` | 0x0004 | Read peinit's own status: how this boot went, with `boot` (§10.2). [*svcsd.control-query-status-grants-the-boot-query] |

The generic mapping:

| Generic right | Maps to |
|---|---|
| `GENERIC_READ` | `SYSTEM_QUERY_STATUS` [*svcsd.control-generic-read-is-query-status] |
| `GENERIC_WRITE` | `SYSTEM_RELOAD_CONFIG` [*svcsd.control-generic-write-is-reload-config] |
| `GENERIC_EXECUTE` | `SYSTEM_SHUTDOWN` [*svcsd.control-generic-execute-is-shutdown] |
| `GENERIC_ALL` | all three [*svcsd.control-generic-all-is-all-three] |

`GENERIC_READ` maps to the one right that changes nothing, as it maps to
`SERVICE_QUERY_STATUS` on a service. Before `boot` existed the control
descriptor governed two actions and no queries, and `GENERIC_READ` on it
conveyed nothing; a descriptor written then that grants `GENERIC_READ`
now lets its holder ask how the machine booted, and nothing more.

## The default [*svcsd.the-control-default]

Absent a value in the registry, peinit applies:

```text
O:SY G:BA D:(A;;0x0007;;;SY)(A;;0x0007;;;BA)(A;;0x0004;;;AU)
```

SYSTEM and Administrators both get every right, as they both get every
service right in the ServiceSecurity default (§4.6). An administrator
who can stop services one at a time can already stop the system, so
withholding shutdown would be theater.
[*svcsd.the-control-default-grants-system-and-administrators-both]

Authenticated Users get `SYSTEM_QUERY_STATUS` and nothing else.
<!-- lint-disable-next-line line-length -->
[*svcsd.the-control-default-lets-every-authenticated-user-ask-how-the-machine-booted]
How a machine booted is machine status, like what `/proc` shows, not a
secret; and it is what a person signed in to the machine needs to tell
that it came up in Safe mode, or is a failed boot or two from recovery.
The ServiceSecurity default lets the same group query every service for
the same reason.

The default applies only while `ControlSecurity` is absent or
malformed. A descriptor already written to the registry is used as it
stands: one written before `SYSTEM_QUERY_STATUS` existed grants it only
where it grants `GENERIC_READ`, `GENERIC_ALL` or the bit itself, and a
machine that wants every signed-in user to read its boot status adds an
ACE for them.

## Loading [*svcsd.controlsecurity-is-loaded-at-phase-2-and-hot-reloaded]

peinit loads the descriptor during Phase 2 boot and hot-reloads it on
registry change notification, on the same path as the service
descriptors. Until it is loaded — during Phase 1 and the early part of
Phase 2 — the built-in default applies, which matters because the
control socket exists from Phase 1 infrastructure setup onwards.
