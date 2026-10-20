---
title: Manage Services and Jobs
type: concept
description: Check a service, start or stop it, find its logs, and choose the next troubleshooting or configuration step.
related:
  - peios/services-and-jobs/defining-a-service
  - peios/services-and-jobs/the-service-lifecycle
  - peios/services-and-jobs/controlling-services
  - peios/services-and-jobs/boot-and-boot-modes
  - peios/peinit/boot/phase-1
  - kernel/internals/kacs/tokens/the-token-model
  - peios/sdk-registry/overview
---

Use **Services Manager** on the desktop or **svctl** in a terminal to see what
is running and control it. Both talk to **peinit**, the service manager that
runs as PID 1. Start with a status check; change the definition only when the
running service needs different configuration.

```sh
svctl list
svctl status jellyfin
svctl boot
```

Replace `jellyfin` with the service name you want to inspect. `list` shows
only services you have permission to query.

## Where to start

| What you need | Start here |
|---|---|
| Check, start, stop, restart, or reload a service | [Controlling services](~peios/services-and-jobs/controlling-services) |
| Understand Failed, Backoff, Skipped, or Abandoned | [Read service states](~peios/services-and-jobs/the-service-lifecycle) and [Troubleshooting](~peios/services-and-jobs/troubleshooting) |
| Find output from a failed run or hook | [Service output and logging](~peios/services-and-jobs/output-and-logging) |
| Create a service or change its settings | [Defining a service](~peios/services-and-jobs/defining-a-service) |
| Change automatic starts or a schedule | [Triggers and timers](~peios/services-and-jobs/triggers-and-timers) |
| Diagnose a reduced boot or reboot loop | [Boot modes](~peios/services-and-jobs/boot-and-boot-modes) |
| Shut down or reboot deliberately | [Shutdown](~peios/services-and-jobs/shutdown) |

For exact field types and defaults, use the [Registry key
reference](~peios/services-and-jobs/registry-key-reference). For
implementation and protocol details, use the [peinit Technical Reference
Manual](~peios/advanced-peios/peinit).

## Three kinds of object

| In status or logs | Meaning | What to inspect |
|---|---|---|
| **Service** | A named definition and its current state. | `svctl status <service>` |
| **Job** | One execution of a service, hook, health check, or submitted program. A restart gets a new job ID. | The job ID in status and the matching logs. |
| **Operation** | One request to start, stop, restart, reload, or reset a service. | `svctl operation-status <id>` when a command returned without waiting. |

See [Jobs and operations](~peios/services-and-jobs/jobs-and-operations) to
follow a run or submit a one-off job.

## peinit in one sentence

peinit starts services from registry definitions, runs each under its
configured identity, supervises it, and stops services in dependency order at
shutdown. You use `svctl` or Services Manager to request those actions.

## What peinit is not

Service definitions live under `Machine\System\Services\` in the registry.
There are no systemd unit files or Windows SCM commands to edit or translate.
Use [Defining a service](~peios/services-and-jobs/defining-a-service) for
configuration and [Service
types](~peios/services-and-jobs/service-types#no-forking-daemons) for programs
that need to stay in the foreground.

Logs and durable history belong to eventd. Open **Logs…** for a service in
Services Manager, or follow [Service output and
logging](~peios/services-and-jobs/output-and-logging). The [TRM compatibility
notes](~peios/advanced-peios/peinit/introduction/service-manager-boundaries#what-peinit-is-not)
describe the interface boundaries.

## The one operational invariant

peinit uses a cached service configuration. A saved change can wait for the
next start or another relevant operation; it does not necessarily change a
running process. Check [when configuration changes
apply](~peios/services-and-jobs/defining-a-service#field-mutability-when-a-change-takes-effect).
The [event-loop
design](~peios/advanced-peios/peinit/introduction/service-manager-boundaries#the-one-operational-invariant)
explains why.

## Where peinit sits in the system

The service's **Identity** controls what its process can access. Its
**ServiceSecurity** permissions control who can manage it. Permission to edit
its registry definition is separate again. See [Service
identity](~peios/services-and-jobs/identity-and-privileges) and [Who can
manage a service](~peios/services-and-jobs/who-can-manage-a-service). The [TRM
system
map](~peios/advanced-peios/peinit/introduction/service-manager-boundaries#where-peinit-sits-in-the-system)
covers the components behind these checks.
