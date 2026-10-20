---
title: The Pre-Exec Sequence
description: Everything between deciding to start a service and its binary running — timeouts, runtime directories, hooks,  and the fork.
---

Everything between "peinit decides to start service X" and "X's binary
is running". The service is in Starting throughout.

<!-- lint-disable-next-line line-length -->
## Step 1: Arm the start timeout [*preexec.one-deadline-covers-the-whole-sequence]

`StartTimeout` covers the entire remaining sequence — pre-hooks, fork,
exec, and the readiness wait. A single deadline, measured from the
operation's creation rather than from this point, bounds all of it.

Expiry aborts the start and kills the service's cgroup tree. The cause
recorded depends on where the deadline landed
[*preexec.the-start-timeout-cause-depends-on-where-it-landed]: a timeout
during pre-hooks is `PreHookFailure`; one during the readiness wait or
the main process's execution is `ReadinessTimeout`.

<!-- lint-disable-next-line line-length -->
## Step 2: Provision runtime directories [*preexec.runtime-directories-are-created-under-run]

If the definition lists `RuntimeDirectories`, peinit creates each one
under `/run` and applies its descriptor (§3.3) before anything else in
the launch. Failure classifies as `ParentSetupFailure`.

<!-- lint-disable-next-line line-length -->
## Step 3: Run pre-exec hooks [*preexec.pre-hooks-run-in-sequence-into-the-hooks-cgroup]

Each `ExecStartPre` command runs in sequence, forked into `hooks/`. A
token is materialised for each at the point of use, from `HookIdentity`
if set and `Identity` otherwise (§4.1); a failure there fails the hook
and the service with `PreHookFailure`.

Any hook exiting non-zero kills the entire service cgroup tree —
cleaning up whatever grandchildren the hook left — and fails the service
with `PreHookFailure`. [*preexec.a-hook-exiting-non-zero-fails-the-service]

When every hook has succeeded, peinit kills the `hooks/` sub-cgroup
before the main process starts, so a hook that forked something into the
background does not become part of the service.
[*preexec.the-hooks-cgroup-is-killed-before-the-main-process]

<!-- lint-disable-next-line line-length -->
## Step 4: Materialise the service token [*preexec.a-token-failure-is-a-parent-setup-failure]

The main process's token, per §4.1. A failure means no child exists and
the service fails with `ParentSetupFailure`.

If a later parent-side step fails before the fork, peinit closes the
token descriptor exactly once. A failure to close is retained as cleanup
evidence and does not change how the start is classified — the
classification describes what went wrong with the start, and a failed
close is a separate fact about the same failure.

<!-- lint-disable-next-line line-length -->
## Step 5: Create the cgroups and the error pipe [*preexec.the-error-pipe-carries-post-fork-failures]

The `main/` and `health/` sub-cgroups are created, and a
`pipe2(O_CLOEXEC)` error pipe. The parent keeps the read end,
non-blocking; the child will hold the write end.

The pipe is how the child reports a failure it hits after fork but
before exec. If exec succeeds, the write end closes automatically —
that is what `O_CLOEXEC` is for — and the parent reads EOF, which means
setup succeeded. If setup fails, the child writes a structured error
first.

**The payload is exactly eight bytes, written with one `write(2)`:**
[*preexec.the-error-payload-is-eight-bytes]

| Bytes | Content |
|---|---|
| 0–3 | little-endian `u32`, the child setup step identifier |
| 4–7 | little-endian `i32`, a positive Linux errno |

The child writes at most one payload — the first reportable failure —
and then exits. The parent treats a non-EOF payload that is not exactly
eight bytes, or carries an unknown step identifier, or an errno of zero
or less, as malformed evidence and fails closed with `PreExecFailure`.
[*preexec.malformed-evidence-fails-closed]

The step identifiers: [*preexec.the-child-setup-step-identifiers]

| Id | Step |
|---|---|
| 1 | Close the read end of the error pipe |
| 2 | Set the standard streams |
| 3 | Reset the signal environment |
| 4 | Install the service token |
| 5 | Set the resource limits |
| 6 | Set `oom_score_adj` |
| 7 | Set the working directory |
| 8 | *reserved* |
| 9 | Confirm `NOTIFY_SOCKET` |
| 10 | Inject stored descriptors |
| 11 | Exec the binary |
| 12 | Create a session |
| 13 | Acquire the controlling terminal |

Identifier 8 is reserved and never emitted
[*preexec.identifier-8-is-reserved]: the environment is built in the
parent and applied by `execve`, so there is no step in the child that
could fail. Identifiers 12 and 13 are the two terminal steps, which
occur only for a service with a `TTYPath`.

If `pipe2` fails, no child exists and the service fails with
`ParentSetupFailure`.

## Step 6: Fork [*preexec.the-child-is-cloned-straight-into-the-main-cgroup]

`clone3(CLONE_PIDFD | CLONE_INTO_CGROUP)`, targeting `main/`. This does
two things atomically: it returns a pidfd for the child, and it places
the child directly into `main/` at creation. There is no window in which
the child exists without a pidfd, and none in which it runs or execs in
peinit's own cgroup.

Placing the child at creation also avoids the alternative, which would
be having the child write its own PID into `cgroup.procs` — something
its post-installation token could not do.

A `clone3` failure means no child exists: `ParentSetupFailure`, with
`EMFILE` /`ENFILE`, `EAGAIN` and `ENOMEM` the usual causes.

peinit does not take the atomicity on trust. If `clone3` reports success
without setting the pidfd, or the pidfd cannot be made close-on-exec,
peinit writes to `cgroup.kill` through the `main/` directory descriptor
before returning `ParentSetupFailure` — because by then a child does
exist, running in the service's cgroup with the service's token, and the
parent has no handle on it. Walking away would leave a process peinit
cannot supervise, cannot stop, and has no record of, which the
zero-delay cleanup that follows a `ParentSetupFailure` would then record
as a leaked tree rather than kill.

Immediately after `clone3` returns, in the parent and on either outcome,
peinit closes the token descriptor and the `main/` cgroup descriptor,
exactly once each. The child relies on close-on-exec and its own exit
instead. A failure to close is cleanup evidence and does not change how
the start is classified.

## Step 7: The parent after the fork

1. Close the write end of the error pipe.
2. Register the read end with the event loop. peinit does not block on
   it — signals, control traffic, timers, and log pipes stay serviceable
   while the child's setup is pending.
3. When the read end becomes readable or hangs up, read it
   non-blocking:
   - **Would block:** leave the source registered, the job stays in
     pending setup.
   - **EOF:** exec succeeded. Record the pidfd on the job, emit
     `peinit.job.started`, and only then apply readiness side effects such as
     Simple/Alive activation.
   - **Data:** setup failed. Parse the step and errno, log the specific
     failure, fail the service with `PreExecFailure`.
     [*preexec.a-child-setup-failure-is-a-pre-exec-failure]

While setup is pending the job is not Running, and no dependent that
waits on this service's readiness is released.

## Steps 8 and 9

The child's own path is §5.4. What follows exec is the readiness wait
and the post-readiness work:

- **Simple with `Readiness=Notify`:** wait for `READY=1`, then Active.
- **Simple with `Readiness=Alive`:** Active as soon as exec succeeds.
- **Oneshot:** wait for the exit. Success is Completed; without
  `RemainAfterExit` it then goes Inactive once dependents are released.
  A non-zero exit is Failed.

On readiness or successful exit, peinit runs `ExecStartPost` in sequence
into `hooks/`, releases the service's dependents, kills `hooks/`, and —
for a Simple service only — arms the watchdog if `WatchdogTimeout` is
non-zero and the health check timer if `HealthCheck` is set.
[*preexec.post-hooks-run-after-readiness]

A post-hook that fails is logged and does not fail the service.
[*preexec.a-failed-post-hook-does-not-fail-the-service] The state
transition to Active or Completed happens before the post-hooks run, so
a service is already Active while its post-hooks are executing.
[*preexec.the-state-transition-precedes-the-post-hooks]

## Failures before the fork

Steps 2, 4, 5, and 6 can all fail with no child in existence. peinit
handles all four entirely in the parent: clean up whatever cgroups were
created, fail the service with `ParentSetupFailure`, and return the
error to the caller. These are system-level resource problems — file
descriptor limits, PID limits, memory, cgroup filesystem errors — rather
than anything about the service.
