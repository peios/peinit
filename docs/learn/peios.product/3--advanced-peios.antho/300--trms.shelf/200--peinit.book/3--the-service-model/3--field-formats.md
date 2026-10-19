---
title: Field Formats
description: How a field's value must be written, as distinct from what it means — strings, identity, privileges, directories,  and environment.
---

Rules that apply to how a field's value is written, as distinct from
what it means.

## Strings

A string field that is present is non-empty, unless this section says
otherwise for that field. [*fmt.a-present-string-field-is-non-empty]
Three fields treat the empty string as absence:
[*fmt.three-fields-treat-the-empty-string-as-absence]

- `Identity` — empty is the same as absent, and defaults to
  `LocalService`.
- `HookIdentity` — empty is the same as absent, and falls back to the
  service's `Identity`.
- `DisplayName` and `Description` — empty is the same as absent.

`WorkingDirectory`, when present, is a non-empty absolute path.
[*fmt.workingdirectory-is-a-non-empty-absolute-path] Whether it exists,
is a directory, and is reachable is checked when the service starts, not
when the definition is read.
[*fmt.workingdirectory-existence-is-checked-at-start-not-at-read]

`TTYPath` is checked for emptiness before absoluteness: an empty value
means no terminal, and a non-empty relative path is rejected.
[*fmt.an-empty-ttypath-means-no-terminal-and-a-relative-one-is-rejected]

## Identity and HookIdentity

Either a well-known principal name — `SYSTEM`, `LocalService`,
`NetworkService` — matched case-insensitively and canonicalised,
[*fmt.a-well-known-identity-is-matched-case-insensitively], or a literal
SID string such as `S-1-5-18`. Anything else is passed to authd verbatim
to resolve (§4.3).

## RequiredPrivileges

Privilege names, matched **case-sensitively** against the published
privilege table. A name that does not match exactly fails token
materialisation and therefore the service start, so `SeTCBPrivilege`
does not start a service that `SeTcbPrivilege` would.
[*fmt.privilege-names-are-matched-case-sensitively]

## RuntimeDirectories

Each entry names one directory directly under `/run`. Entries are
non-empty relative names; an entry equal to `.` or `..` is rejected, as
is any entry containing `/`, `\`, a NUL, or a control character. A dot
inside a name is fine — `app.sock.d` is a valid entry.
[*fmt.a-runtime-directory-entry-is-one-relative-name]

For an entry `foo`, peinit creates `/run/foo` immediately before
launching the service's main process, with a descriptor granting full
access to SYSTEM, Administrators, and the service's own SID. Hook
processes inherit the service's environment and identity rules but do
not cause provisioning — the directories belong to the main start.
[*fmt.hooks-do-not-cause-runtime-directory-provisioning]

If creation or descriptor assignment fails, the start fails with
`ParentSetupFailure`.
[*fmt.a-failed-runtime-directory-is-a-parent-setup-failure]

peinit does not remove runtime directories when a service stops.
[*fmt.runtime-directories-are-not-removed-when-a-service-stops] `/run`
is a boot-scoped tmpfs and the next boot clears it.

## Environment

Each entry is `KEY=VALUE`, with a non-empty key containing no NUL. An
entry that does not split into that shape is a decode error.
[*fmt.an-environment-entry-is-key-equals-value]

## SuccessExitCodes

Each entry is a decimal integer from 0 to 255 — a process exit code.
Signal names and ranges are not accepted.
[*fmt.a-success-exit-code-is-a-decimal-integer-0-to-255] Code 0 is
always success and does not need listing. Duplicates are collapsed.

## Dependency and handler names

Entries in `Requires`, `Wants`, `BindsTo` and `Conflicts`, and the
value of `OnFailure`, are validated when the definition is read. A
dependency naming something outside `[A-Za-z0-9._-]` is a decode error
rather than an unresolved dependency discovered later
[*fmt.an-illegal-dependency-name-is-a-decode-error] — the difference
being that a typo containing an illegal character is caught immediately,
while a typo that is still a legal name is caught at graph validation as
a missing target.

An entry in the four list fields can carry a readiness level, written
after a colon (`netd:routed`). The name and the level are validated
separately, so the colon is not one of the illegal characters there.
<!-- lint-disable-next-line words spelling -->
[*fmt.a-list-dependency-may-carry-a-level] `OnFailure` takes a bare
service name and no level: it names a handler to run, not a state to
wait for. [*fmt.onfailure-takes-no-level]

## Timeouts and intervals

Every timeout and interval in the schema is in whole seconds unless the
field says otherwise. [*fmt.timeouts-and-intervals-are-in-whole-seconds]
The two `sd_notify` fields that carry durations,
`WATCHDOG_USEC` and `EXTEND_TIMEOUT_USEC`, are in microseconds because
that is what the protocol specifies.

## Identifiers peinit generates

Job identifiers, operation identifiers, and every other GUID peinit
mints are UUIDv7. [*fmt.generated-identifiers-are-uuidv7] UUIDv7 is
time-ordered, so identifiers sort by creation time
[*fmt.identifiers-sort-by-creation-time] — which is what keeps eventd's
time-range and recency
queries over jobs and operations cheap.
