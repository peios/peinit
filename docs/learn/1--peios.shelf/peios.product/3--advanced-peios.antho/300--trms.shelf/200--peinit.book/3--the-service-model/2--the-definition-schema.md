---
title: The Definition Schema
description: Every field a service definition can carry, with its registry type and default, and why decoding is all-or-nothing.
---

Every field a service definition can carry, with its registry type and
its default. [*schema.the-field-table] The semantics of each are in the
section named alongside.

Value names are matched case-insensitively, so `ImagePath` and
`imagepath` are the same field
[*schema.value-names-are-matched-case-insensitively]
— and therefore a definition carrying both is a duplicate, not two
fields.

| Field | Type | Default | Meaning |
|---|---|---|---|
| ImagePath | string | *required* | Absolute path to the service binary. |
| Arguments | multi_string | — | Arguments passed to the binary. |
| Type | dword | 0 (Simple) | 0 Simple, 1 Oneshot. §3.1 |
| Triggers | multi_string | — | When the service starts automatically. §3.4 |
| Disabled | dword | 0 | If 1, no trigger activates the service. |
| SafeMode | dword | 0 | If 1, attempt this service in Safe mode. Implied by `ErrorControl=Critical`. §2.6 |
| Identity | string | LocalService | `SYSTEM`, shared `LocalService`/`NetworkService`, `Service` for the service's own virtual account, or a designated principal. §4.1 |
| RequiredPrivileges | multi_string | — | Privileges to keep; all others are removed. §4.5 |
| Requires | multi_string | — | Hard dependencies. §7.1 |
| Wants | multi_string | — | Soft dependencies. §7.1 |
| BindsTo | multi_string | — | Runtime coupling. §7.1 |
| Conflicts | multi_string | — | Mutual exclusion. §7.1 |
| Provides | multi_string | — | Roles this service fills: virtual names a dependency can target (`network`), and that peinit derives edges to. A role is a bare name — the level in `network:routed` comes from the depending service's readiness requirement, not from this value. §7.6 |
| OnFailure | string | — | Service to start when this one fails. §6.3 |
| ErrorControl | dword | 0 (Normal) | 0 Normal, 1 Critical. |
| RemainAfterExit | dword | 0 | Oneshot only: stay Completed after a successful exit. |
| SuccessExitCodes | multi_string | — | Non-zero exit codes treated as success. |
| ExecStartPre | multi_string | — | Commands run before the main binary, sequentially. §5.3 |
| ExecStartPost | multi_string | — | Commands run after readiness or successful exit. §5.3 |
| HookIdentity | string | — | Principal for the hook processes. Falls back to `Identity`. §4.1 |
| ExecReload | string | — | Reload command, or `signal:<NAME>`. Absent means SIGHUP. §6.5 |
| PreStartCheckTimeout | dword | 5 | Seconds before a filesystem check helper is killed. §3.5 |
| StartTimeout | dword | 30 | Seconds for the entire start sequence. §5.3 |
| StopTimeout | dword | 10 | Seconds after SIGTERM before SIGKILL. |
| WatchdogTimeout | dword | 0 | Seconds between expected `WATCHDOG=1` pings; 0 disables. §6.6 |
| HealthCheck | string | — | Command run periodically. Exit 0 is healthy. §5.6 |
| HealthCheckInterval | dword | 30 | Seconds between health checks. |
| HealthCheckTimeout | dword | 5 | Seconds before a health check is killed and counted failed. |
| HealthCheckRetries | dword | 3 | Consecutive failures before the service is unhealthy. |
| RestartPolicy | dword | 1 (OnFailure) | 0 Never, 1 OnFailure, 2 Always. §6.4 |
| RestartMaxRetries | dword | 5 | Consecutive restarts before Failed. §6.4 |
| RestartWindow | dword | 120 | Seconds of sustained health that reset the restart counter. |
| RestartDelay | dword | 1 | Seconds before a restart; doubles each consecutive failure, capped at 60. |
| Readiness | dword | 0 (Notify) | 0 Notify, 1 Alive. Ignored for Oneshot. |
| NotifyAccess | dword | 0 (Main) | Who can send notifications. Main is the only mode. §10.5 |
| FdStoreMax | dword | 0 | Maximum descriptors held for the service; 0 disables the store. §10.6 |
| TimerPersistent | dword | 1 | Catch up a missed timer run after a reboot. §9.3 |
| TimerJitter | dword | 0 | Maximum random delay added to each firing. §9.4 |
| Environment | multi_string | — | `KEY=VALUE` pairs added to the environment. §5.5 |
| WorkingDirectory | string | `/` | Working directory for the process. |
| TTYPath | string | — | Terminal to attach as the standard streams and controlling terminal. §5.4 |
| TTYPrecedence | dword | 0 | Who wins that terminal when several services want it at once. Higher wins. Refused without a `TTYPath`. §11.6 |
| RuntimeDirectories | multi_string | — | Private directories under `/run`, created before the main process. |
| LimitNOFILE | dword | — | `RLIMIT_NOFILE`. |
| LimitCORE | dword | — | `RLIMIT_CORE`, in bytes. |
| Conditions | multi_string | — | Start-time conditions; failure skips the service. §3.5 |
| Asserts | multi_string | — | Start-time assertions; failure fails the service. §3.5 |
| DisplayName | string | — | Human-readable name for status display. |
| Description | string | — | What the service does. |
| ServiceSecurity | binary | inherit | Descriptor controlling runtime operations on the service. §4.6 |

## Registry types

| Schema type | Registry type |
|---|---|
| string | `REG_SZ`, UTF-8 |
| multi_string | `REG_MULTI_SZ`, an ordered list |
| dword | `REG_DWORD`, 32-bit unsigned |
| binary | `REG_BINARY` |

A value whose registry type does not match the field's is a decode
error. [*schema.a-type-mismatch-is-a-decode-error] So is a dword
carrying a value outside an enumerated field's range.
[*schema.a-dword-outside-an-enumerated-range-is-a-decode-error]

## Schema version and forward compatibility

`Machine\System\Services\SchemaVersion` is a dword, 1.
[*schema.the-services-schema-version-is-one] peinit creates it if it is
absent (§2.3).

Unknown values on a service key are ignored,
[*schema.an-unknown-value-on-a-service-key-is-ignored] which is what
lets the schema grow additively: a definition written for a newer peinit
still loads on an older one, minus the fields it does not understand. A
newer schema version does not prevent boot.
[*schema.a-newer-schema-version-does-not-prevent-boot]

Known fields are the opposite. A known field appearing more than once in
a collected definition is a decode error rather than a last-one-wins,
[*schema.a-duplicate-known-field-is-a-decode-error] because a definition
that says two different things about the same field has no defensible
reading. Since names match case-insensitively, this catches `ImagePath`
and `imagepath` in the same key.

## What a decode failure costs

A definition that fails to decode fails that one service, and the answer
differs by caller.

At boot the key is marked Failed with cause `ValidationError` and the
boot proceeds with every other definition. Anything that depended on the
failed service fails in turn through the ordinary dependency propagation
(§7.4), so the cost is bounded by what actually needed it.

On reload-config the same: that service is Failed with cause
`ValidationError` — or, if it is running, left running with its
definition marked removed — and every other definition loads (§10.4).
[*schema.a-decode-failure-fails-only-that-service-on-reload]
