---
title: Registry Key Reference
description: Every registry key peinit reads or writes — service definitions, boot configuration, operational parameters,  and the watches it holds.
---

Every registry key peinit reads or writes. Semantics are in the sections
referenced.

## Service definitions

| Key | Purpose | Defined in |
|---|---|---|
| `Machine\System\Services\` | Parent key. Each child key is one service. | §3.2 |
| `Machine\System\Services\SchemaVersion` | Schema version guard. `REG_DWORD`, 1. Created by peinit if absent. | §2.3, §3.2 |
| `Machine\System\Services\ServiceSecurity` | The descriptor inherited by definitions that carry none. `REG_BINARY`. | §4.6 |
| `Machine\System\Services\<name>` | One service definition. | §3.2 |
| `Machine\System\Services\<name>\Provides` | Roles this service fills, which dependencies can name in place of the service. `REG_MULTI_SZ`. Read at boot and on reload. | §7.6 |
| `Machine\System\Services\<name>\LastTimerRun` | Last-run timestamp for a single-trigger persistent timer. `REG_QWORD`, written by peinit. | §9.3 |
| `Machine\System\Services\<name>\TimerState\` | Per-trigger timestamps for a multi-trigger service. Each value is named by the percent-encoded schedule and holds a `REG_QWORD`. | §9.3 |

## Boot configuration

| Key | Type | Default | Purpose | Defined in |
|---|---|---|---|---|
| `Machine\System\Boot\MaxParallelStarts` | dword, > 0 | 10 | Services starting concurrently during boot. Zero, a type mismatch, or a malformed payload is invalid and enters recovery. | §2.5 |
| `Machine\System\Boot\BootSuccessGrace` | dword | 30 | Seconds every Critical service has to hold a dependent-satisfying state before the boot counts as successful. | §2.5 |
| `Machine\System\Boot\SettleTimeout` | dword | 5 | Seconds to wait for the boot set to settle before starting `boot:settled` services regardless. Zero is legal. | §2.5 |
| `Machine\System\Boot\ShutdownTimeout` | dword | 90 | Seconds for the entire graceful shutdown. | §12.2 |
| `Machine\System\Boot\PostKillTimeout` | dword | 5 | Seconds for a cgroup to drain after SIGKILL before it is treated as stuck. Bounds one service's final stop, where `ShutdownTimeout` bounds the sequence. | §12.2 |

## Operational parameters

| Key | Type | Default | Purpose | Defined in |
|---|---|---|---|---|
| `Machine\System\Init\ControlSecurity` | binary | SYSTEM and Administrators, every right; Authenticated Users, `SYSTEM_QUERY_STATUS` | The descriptor for system-level control operations and the boot query. | §4.7 |
| `Machine\System\Init\MaxControlConnectionsPerUser` | dword | 16 | Concurrent control socket connections one caller's user SID can hold. SYSTEM is exempt. | §10.1 |
| `Machine\System\Init\MaxControlConnections` | dword | 256 | Concurrent control socket connections in all. | §10.1 |
| `Machine\System\Init\MaxRequestSize` | dword | 65536 | Maximum control request size, in bytes. | §10.1 |
| `Machine\System\Init\ConnectionTimeout` | dword | 30 | Seconds before an idle control connection is closed. | §10.1 |
| `Machine\System\Init\MaxJobsConnections` | dword | 64 | Concurrent jobs socket connections. | §10.7 |
| `Machine\System\Init\MaxJobMessageSize` | dword | 32768 | Maximum jobs socket message content, in bytes. | §10.7 |
| `Machine\System\Init\JobsConnectionTimeout` | dword | 30 | Seconds before an idle jobs connection is closed. | §10.7 |
| `Machine\System\Init\MaxJobsPerSubmitter` | dword | 64 | Live submitted jobs one submitter SID can hold. SYSTEM is exempt. | §8.5 |
| `Machine\System\Init\MaxLogLineLength` | dword | 8192 | Bytes per output line before truncation. Minimum 256; below that the default is used and a warning logged. | §11.3 |
| `Machine\System\Init\MaxLogBufferPerService` | dword | 65536 | Pipe capacity per service, applied with `F_SETPIPE_SZ`. Minimum 4096 (one page, the kernel's own floor). | §11.3 |
| `Machine\System\Init\LogReadBytesPerEvent` | dword | 16384 | Bytes drained from one output pipe per readable event. Minimum 512. | §11.3 |
| `Machine\System\Init\PreEventdBuffer` | dword | 1048576 | Bytes of output retained before eventd is available. Applied at boot and on reload. Minimum 4096. | §11.2 |
| `Machine\System\Init\EnvVars\` | parent key | empty | Variables injected into every service but registryd. Value name is the variable name; `REG_SZ` data is the value. Its descriptor is security-critical. | §5.5 |
| `Machine\System\Init\ProvisionedPaths\` | parent key | empty | Boot-time path provisioning entries. | §2.4 |
| `Machine\System\Init\ProvisionedPaths\<name>\Kind` | string | — | `directory` or `file`. Required. | §2.4 |
| `Machine\System\Init\ProvisionedPaths\<name>\Path` | string | — | The absolute path. Required. | §2.4 |
| `Machine\System\Init\ProvisionedPaths\<name>\Security` | binary | built-in | The descriptor to apply. | §2.4 |
| `Machine\System\Init\ProvisionedPaths\<name>\Required` | dword | 0 | If 1, a failure enters recovery before Phase 2. | §2.4 |

## Other subsystems

| Key | Type | Purpose | Defined in |
|---|---|---|---|
| `Machine\System\eventd\LogSocketPath` | string | Where peinit forwards service output. | §11.4 |

## Watches

peinit subscribes to `Machine\System\Services\` and
`Machine\System\Init\` at boot. Any drained event triggers a full
reload-config, which also covers the OVERFLOW case (§3.7).
