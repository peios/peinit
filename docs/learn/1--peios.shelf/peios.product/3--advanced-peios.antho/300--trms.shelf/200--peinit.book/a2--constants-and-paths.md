---
title: Constants and Paths
description: Every value compiled into peinit that no registry key changes — paths, timeouts, limits, environment,  and the descriptors it applies.
---

Values compiled into peinit, which no registry key changes.

## Filesystem paths

| Path | Purpose |
|---|---|
| `/usr/bin/peinit2` | Where peinit is installed in package storage. |
| `/bin/peinit2` | The runtime path the kernel `init=` names. |
| `/sbin/registryd` | The compiled-in registryd image path. |
| `/var/state/loregd/Machine.hive` | The machine hive, passed to registryd. |
| `/var/state/loregd/Users.hive` | The users hive, passed to registryd. |
| `/.peinit/` | peinit's own state directory on the root filesystem. |
| `/.peinit/boot-attempts` | The boot attempt counter. |
| `/lcl/etc/machine-id` | The local machine identifier. |
| `/var/state/peinit/random-seed` | The persisted entropy seed. |
| `/lcl/policy/autorun.d` | Phase 1 autorun scripts. |
| `/run/services/peinit/control.sock` | The control socket. |
| `/run/services/peinit/notify.sock` | The notification socket, by default. |
| `/run/services/peinit/jobs.sock` | The jobs socket. |
| `/sys/fs/cgroup/peinit/` | The root of every service cgroup tree. |
| `/sys/fs/cgroup/peinit/jobs/<guid>` | A submitted job's cgroup. |
| `/dev/rtc`, `/dev/rtc0` | The hardware clock, in that order of preference. |
| `/dev/console` | Where peinit writes its own messages. |
| `/bin/recsh`, `/bin/sh` | The recovery shell, in that order of preference. |

## Timeouts and limits

| Constant | Value | Meaning |
|---|---|---|
| registryd setup timeout | 30 s | Process setup, driven synchronously in Phase 1. |
| registryd readiness timeout | 30 s | Waiting for `READY=1` in Phase 1. |
| Reload detection window | 2 s | Waiting for `RELOADING=1` after a reload signal. Bounded above by the operation deadline. |
| Restart delay cap | 60 s | The ceiling on exponential backoff. |
| Timeout extension cap | ×4 | The multiple of a phase's base timeout an extension can reach. |
| Operation retention | 60 s | How long a terminal operation is kept before being dropped. |
| Submitted job retention | 60 s | How long a terminal submitted job's entry answers `status` before being dropped. |
| `peinit.job.status.reported` interval | 1 s | The least spacing between two `peinit.job.status.reported` events for one job. |
| Submitted job `stop_timeout` | 10 s | The default, per submission. |
| Submitted job `readiness_timeout` | 30 s | The default, per submission. |
| Submitted job argv and environment | 2 MiB | The bound on a definition's `arguments` and `environment` together. |
| Boot attempt threshold | 3 | The default, overridden by `peios.bootattempts=`. |
| `OnFailure` chain depth | 16 | The maximum handler chain from one originating failure. |
| Pre-eventd buffer | 1 MiB | The compiled-in buffer capacity. |
| Notification datagram | 64 KiB | The receive buffer size. |
| Descriptors per datagram | 64 | The control message buffer capacity. |
| Random seed | 512 bytes | Written at shutdown. Restoring accepts 1–4096 bytes. |
| Control listen backlog | 32 | |
| Jobs listen backlog | 32 | |
| Descriptors per jobs message | 64 | Including an output sink. |
| Tokens per jobs message | 1 | |
| First injected descriptor | 3 | Where stored descriptors are placed. |
| Final action retry | 1 s | The minimum interval between `reboot(2)` retries. |

## Environment

| Variable | Value |
|---|---|
| `PATH` | `/sbin:/bin` — the compiled-in base for every service. |

The recovery shell additionally receives `TERM=linux` and `HOME=/`.

## Kernel command line

| Parameter | Effect |
|---|---|
| `peios.safemode=1` | Force Safe mode. |
| `peios.recovery=1` | Force recovery mode. |
| `peios.bootattempts=N` | The recovery threshold; `0` disables the check. |
| `peios.quiet=N` | Console verbosity: 0, 1,  or 2. |
| `peios.notifysocket=PATH` | Override the notification socket path. |

## Descriptors peinit applies

| Where | SDDL |
|---|---|
| `/dev/shm`, `/run`, `/sys/fs/cgroup` after mounting | `O:SYG:SYD:(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)(A;CI;GRGX;;;WD)(A;OICIIO;GA;;;S-1-3-0)` |
| `/dev/null`, `/dev/zero`, `/dev/full`, `/dev/random`, `/dev/urandom`, `/dev/tty`, `/dev/ptmx` (DACL only) | `D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;FRFW;;;WD)` |
| A provisioned path with no `Security` | `O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;FR;;;BU)` |
| A service's `/run/<name>` runtime directory | `O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;<service SID>)` |
| The random seed file | `O:SYG:SYD:(A;;GA;;;SY)` |
| The default ServiceSecurity | `O:SYG:BAD:(A;;0x000F;;;SY)(A;;0x000F;;;BA)(A;;0x0001;;;AU)` |
| The default ControlSecurity | `O:SYG:BAD:(A;;0x0007;;;SY)(A;;0x0007;;;BA)(A;;0x0004;;;AU)` |
| The control socket inode | `O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;FW;;;AU)` |
| The jobs socket inode | `O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;FW;;;AU)` |
| A submitted job with no `security_descriptor` | Owner and group the submitter; `(A;;0x0007;;;<submitter>)(A;;0x0007;;;SY)(A;;0x0007;;;BA)` |

## Access rights

| Right | Bit |
|---|---|
| `SERVICE_QUERY_STATUS` | 0x0001 |
| `SERVICE_START` | 0x0002 |
| `SERVICE_STOP` | 0x0004 |
| `SERVICE_INTERROGATE` | 0x0008 |
| `SERVICE_ALL_ACCESS` | 0x000F |
| `SYSTEM_SHUTDOWN` | 0x0001 |
| `SYSTEM_RELOAD_CONFIG` | 0x0002 |
| `SYSTEM_QUERY_STATUS` | 0x0004 |
| `JOB_QUERY` | 0x0001 |
| `JOB_STOP` | 0x0002 |
| `JOB_SIGNAL` | 0x0004 |
| `JOB_ALL_ACCESS` | 0x0007 |

## Identifiers

Every GUID peinit generates — jobs, operations — is UUIDv7, so
identifiers sort by creation time.
