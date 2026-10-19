---
title: Health Checks
description: A watchdog says a service is still ticking; a health check says it still works — execution, overlap, failure,  and the flap constraint.
---

A watchdog tells peinit that a service is still ticking. A health check
tells it that the service still works. They address different failures:
a process can be alive and responsive to its own event loop while having
lost its database connection, wedged in a bad state, or started
returning errors to everyone.

## Execution [*health.the-check-runs-with-the-services-own-token]

The health check command runs with the **service's own token** — never
`HookIdentity` — so it checks the service's health from the service's
own vantage point.

Each invocation runs in an ephemeral `health/` sub-cgroup under the
service's tree, as a child of peinit rather than of the service. When
the check completes or times out, peinit kills the whole sub-cgroup,
which cleans up anything the check spawned.
[*health.an-invocation-runs-in-the-health-cgroup-as-a-child-of-peinit]

## Overlap [*health.an-overlapping-check-is-skipped]

If the previous check is still running when the next interval fires, the
new one is skipped and nothing is counted. A check exceeding
`HealthCheckTimeout` has its sub-cgroup killed and **is** counted as a
failure. [*health.a-timed-out-check-counts-as-a-failure]

A launch failure — a token that could not be materialised, a fork that
failed — is **not** counted. [*health.a-launch-failure-is-not-counted]
The invocation is recorded and the next interval schedules normally, and
nothing about the service changes.

The distinction is between "the probe ran and said the service is
unhealthy" and "the probe could not be run". Only the first is evidence
about the service. Collapsing them meant a transient authd unavailability
could kill a service outright: with `HealthCheckRetries=1`, a reasonable
setting for a probe an operator trusts, one failed token materialisation
exhausted the budget and restarted it.

## Failure [*health.consecutive-failures-restart-the-service]

`HealthCheckRetries` consecutive failures mark the service unhealthy.
An unhealthy service is restarted through the ordinary restart policy:
`RestartPolicy`, exponential backoff and throttling all apply, exactly
as for a crash. The failure count resets the moment a check succeeds.
[*health.a-success-resets-the-failure-count]

Escalation kills the service's **root** cgroup rather than `main/`,
so it takes hooks and probes with it.
[*health.escalation-kills-the-root-cgroup]

## The flap constraint [*health.the-flap-constraint]

Restart throttling is what stops a service flapping — failing checks,
restarting, passing initial checks, failing again. But it only works if
the failure cycle is shorter than `RestartWindow`, because otherwise the
service stays healthy long enough between failures to reset the restart
counter, `RestartMaxRetries` is never reached, and it restarts forever.

So this relationship has to hold:

```text
HealthCheckRetries × HealthCheckInterval < RestartWindow
```

It is enforced as an error rather than a warning, in both places a
definition can arrive. At boot, a violating service is blocked with
cause `ValidationError` and never started.
[*health.a-violating-definition-is-blocked-at-boot] On reload-config, it
is a validation finding, and a finding rejects the entire reload.
[*health.a-violating-definition-rejects-a-reload]

The constraint applies only where a check will actually run, which means
Simple services. [*health.the-constraint-applies-only-to-simple-services]
A Oneshot cannot flap through health checks because it never runs one.

A Oneshot that declares a `HealthCheck` is still rejected — but for
declaring one at all, not for its timing.
[*health.a-health-check-on-a-non-simple-service-is-rejected] That is the
thing actually wrong with the definition; reporting it as timing
arithmetic let an operator adjust `RestartWindow`, see the definition
validate, and still have a `HealthCheck` that does nothing.

> [!NOTE]
> Active health checks on `ErrorControl=Critical` services deserve
> caution. A false positive eventually exhausts the restart budget and
> reboots the machine. Critical services — registryd, authd, lpsd,
> eventd — are usually better served by a passive watchdog with a
> generous timeout: the service knows whether it is healthy and can stop
> sending keepalives when it decides it is not. peinit does not prevent
> health checks on Critical services and applies identical semantics to
> them, but the escalation path ends at a reboot.
