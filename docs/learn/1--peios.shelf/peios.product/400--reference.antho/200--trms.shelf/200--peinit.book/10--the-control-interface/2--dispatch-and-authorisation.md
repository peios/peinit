---
title: Dispatch and Authorization
description: The fixed sequence a parsed command runs before it does anything — the shutdown gate, the rights checked, and what is filtered rather than denied.
---

A parsed command runs a fixed sequence before it does anything.

1. **The shutdown gate.** If peinit is shutting down, everything except
   `status`, `list`, `operation-status`, `boot`, `job-status`,
   `job-list` and `job-stop` is rejected. [*dispatch.the-shutdown-gate]
   `boot` stays open because, like the service queries, it changes
   nothing. [*dispatch.boot-passes-the-shutdown-gate] The three job
   commands stay open because a
   shutdown stops every submitted job anyway (§12.2), and an
   administrator watching that happen, or wanting a job gone sooner
   than the shutdown will manage, has a reason to ask. The gate runs
   before the access check, so during shutdown a caller who would have
   been denied is told the command is invalid for the current state
   rather than that they lack the right.
   [*dispatch.the-gate-runs-before-the-access-check]
2. **Resolve the target.** A command naming no definition, and no
   addressable definition-removed entry, returns `UNKNOWN_SERVICE`;
   one naming no submitted job peinit holds returns `UNKNOWN_JOB`.
   [*dispatch.an-unresolvable-target-is-unknown] peinit does not
   synthesise a descriptor for something that does not exist.
3. **AccessCheck.** The caller's token, the target's descriptor, the
   generic mapping, the right the command requires (§4.6, §4.7), and an
   audit context naming the target: the service, the job, or the
   control door itself (§8.4). For a job the descriptor is the job's own
   and the mapping is the job mapping (§8.5).
   [*dispatch.the-access-check-inputs]
4. **On denial**, return `ACCESS_DENIED`. The decision is recorded by
   KACS, as `kacs.audit.access.checked` naming the caller, the target
   and the access requested and granted, whenever the target's
   descriptor has a SACL that asks for it; every descriptor peinit
   builds itself audits every refusal. peinit writes no event of its
   own. [*dispatch.a-denial-is-answered-and-audited] Silent denial is
   not acceptable; a denial an administrator cannot see is
   indistinguishable from a bug, which is why the defaults audit, and
   why a descriptor written to the registry should keep a SACL that
   does (§4.6).
5. **On grant**, classify the command against the service's state
   (§10.3) and act.

## Rights

| Command | Right |
|---|---|
| `start` | `SERVICE_START` [*dispatch.right-start] |
| `stop` | `SERVICE_STOP` [*dispatch.right-stop] |
| `restart` | `SERVICE_START` and `SERVICE_STOP`, requested as one mask [*dispatch.right-restart] |
| `reload` | `SERVICE_INTERROGATE` [*dispatch.right-reload] |
| `reset` | `SERVICE_STOP` [*dispatch.right-reset] |
| `status` | `SERVICE_QUERY_STATUS` [*dispatch.right-status] |
| `list` | Filtered per service by `SERVICE_QUERY_STATUS` [*dispatch.right-list] |
| `operation-status` | `SERVICE_QUERY_STATUS` on the target service [*dispatch.right-operation-status] |
| `job-status` | `JOB_QUERY` on the job [*dispatch.right-job-status] |
| `job-list` | Filtered per job by `JOB_QUERY` [*dispatch.right-job-list] |
| `job-stop` | `JOB_STOP` on the job [*dispatch.right-job-stop] |
| `shutdown` | `SYSTEM_SHUTDOWN` [*dispatch.right-shutdown] |
| `reload-config` | `SYSTEM_RELOAD_CONFIG` [*dispatch.right-reload-config] |
| `boot` | `SYSTEM_QUERY_STATUS` on the control descriptor, which by default every authenticated principal holds (§4.7) [*dispatch.right-boot] |

`operation-status` resolves the operation before it checks the right, so
an unknown identifier is reported as unknown regardless of who asked.
[*dispatch.operation-status-resolves-before-it-checks]
If the operation's service has since been discarded (§3.8), the right is
checked against the `ServiceSecurity` the service had when the operation
was created, which peinit records on the operation; a retained operation
therefore stays queryable for its retention window (a2) by exactly the
callers who could have queried the service.
[*dispatch.a-retained-operation-keeps-its-services-descriptor]

`job-status` and `job-stop` resolve the job first too, and for them
that is the whole story: `UNKNOWN_JOB` for an identifier that names
nothing, `ACCESS_DENIED` for one that names a job the caller can not
touch. A job identifier is unguessable, so a caller that names one was
told it by something that had it, and there is no existence to hide.
`job-stop` creates no operation; it stops the job directly (§8.5) and,
with `wait`, registers a job wait on the connection answered by the
terminal view. [*dispatch.job-stop-creates-no-operation]

## What the caller can do

A `status` response carries `granted`, the service rights the caller
holds on the service, so that a client can offer only the commands that
will be allowed rather than learn the rest from `ACCESS_DENIED` (PSPU
§4.14). peinit finds them with step 3's AccessCheck — the caller's
token, the service's `ServiceSecurity`, the service mapping — asking
for `MAXIMUM_ALLOWED` instead of one right, once the query itself has
been allowed, and reports the service rights among what it grants.
[*dispatch.status-reports-the-callers-rights] A lifecycle command that
has nothing to do answers with the status (§10.3), and that carries
`granted` for its caller too.
[*dispatch.a-status-answering-a-command-reports-the-callers-rights]

Each job view on the control socket carries `granted` the same way:
the job rights the caller holds, from the AccessCheck against the job's
own descriptor with the job mapping (§8.5). `job-stop` with `wait`
finds them when it is taken and keeps them on the connection's wait,
because the terminal view is written by the run loop with no caller's
token to hand; the descriptor is fixed at submission, so they are the
same as at the answer. The jobs socket's view has no `granted`.
[*dispatch.job-views-on-the-control-socket-report-the-callers-rights]

The `MAXIMUM_ALLOWED` check is a question, not a command. A right it
does not grant is left out of `granted`; nothing is refused, and peinit
answers the command as it would have without it.
[*dispatch.a-maximum-allowed-check-is-not-a-denial]

## Filtering

`list` checks every service and partitions the result. Services the
caller can query are returned; services it cannot are omitted, and the
denials are left to each service's SACL rather than shown to the caller. A
caller with no query rights anywhere gets an empty list and a successful
response, not a denial [*dispatch.list-filters-rather-than-denies]. The
filtering keeps a service's state from whoever can not query it; it does
not hide that the service exists, which step 2 above already answers
for any command that names one (§4.6).

Definition-removed services are listed, and the list entry does not say
so. A `status` query on one does.
[*dispatch.a-definition-removed-service-is-listed-without-saying-so]

`job-list` is filtered the same way. The four request filters —
`submitter`, `identity`, `logon_session`, `state` — narrow the
candidates first; each survivor is then checked for `JOB_QUERY` against
its own descriptor, each denial is the job's SACL's to record, and the
response does not say whether a filter or a right removed an entry.
[*dispatch.job-list-filters-before-it-checks]
Terminal jobs still within their retention are listed; a caller that
wants only live ones filters by `state`.
[*dispatch.terminal-jobs-within-retention-are-listed]
