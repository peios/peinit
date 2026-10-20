---
title: Service Registry Keys
type: reference
description: The registry keys that hold peinit's service definitions, boot settings, and operational parameters, which keys peinit writes itself, and how to look up any value with regman.
related:
  - peios/services-and-jobs/defining-a-service
  - peios/registry/regman
  - peios/registry/changing-a-setting
---

peinit reads its configuration from four places in the registry. This page
says what each one holds and which guide explains it. The registry manual
for each value, shown by `regman`, is the authority for its type, default,
valid range, and when a change takes effect.

## Where peinit's settings live

peinit reads these keys:

| Key | Holds | Guide |
|---|---|---|
| `Machine\System\Services\<name>` | One service definition per subkey. Only `ImagePath` is required. | [Defining a Service](~peios/services-and-jobs/defining-a-service) |
| `Machine\System\Boot` | Boot limits and timeouts: parallel starts, the success grace period, the settle timeout, and shutdown timeouts. | [Boot and Boot Modes](~peios/boot/boot-and-boot-modes) |
| `Machine\System\Init` | Operational parameters: control and jobs socket limits, service output limits, default environment variables, and boot-time path provisioning. | [Operational parameters](#operational-parameters) |
| `Machine\System\eventd` | `LogSocketPath`, the socket that peinit forwards service output to. | [Finding Service Logs](~peios/services-and-jobs/output-and-logging) |

Replace `<name>` with the service's name. peinit learns about edits to
service definitions through a registry watch. Saving a value doesn't mean
that a running service has adopted it; each value's manual says when a
change applies.

## Looking up a value

Use `regman` with the key's path to read the documentation for every value
under it:

```sh
regman 'Machine\System\Services'
```

`regman` reads the installed documentation, not the live registry. Use
[`reg`](~peios/registry/reg) to read the value configured on this machine.
To check a whole definition before peinit uses it, run
`svctl definition validate`.

## Operational parameters

The values under `Machine\System\Init` bound peinit's own resources. They
fall into four groups:

- **Control and jobs sockets.** The security descriptor for system-level
  control operations, connection counts, request and message sizes, idle
  timeouts, and the number of live jobs each submitter can hold.
  See [Controlling Services](~peios/services-and-jobs/controlling-services)
  and [Jobs and Operations](~peios/services-and-jobs/jobs-and-operations).
- **Service output.** Line length, the buffer for each service, and the
  output held before eventd starts. See
  [Finding Service Logs](~peios/services-and-jobs/output-and-logging).
- **`EnvVars`.** Default environment variables for the services peinit
  starts once the registry is available. Protect this key's security
  descriptor: every value written here reaches those services. See
  [The Execution Environment](~peios/services-and-jobs/execution-environment).
- **`ProvisionedPaths`.** Directories and files that peinit creates and
  secures during boot. See [Boot and Boot Modes](~peios/boot/boot-and-boot-modes).

## Values peinit writes

peinit records the last run of each persistent timer, in the `LastTimerRun`
value or the `TimerState` subkey of the service's definition. Don't edit
these values. See [Triggers and Timers](~peios/services-and-jobs/triggers-and-timers).

The [peinit technical reference manual](~peios/peinit/registry-keys) lists
every key from the implementation's side.
