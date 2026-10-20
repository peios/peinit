---
title: Path Provisioning
description: Filesystem objects belonging to no single service — how entries are declared, what descriptors they get, and what failure does.
---

Some filesystem objects belong to no single service. A directory two
packages both write into, a state file created before anything runs, a
path that has to exist with a particular Security Descriptor before the
first service that uses it starts — none of these has an owner in the
service model, and creating them from a service's pre-exec hook makes
their existence depend on start ordering.

Boot-time path provisioning is the registry-backed answer, and the
equivalent of the tmpfiles.d role elsewhere. peinit applies it after
registryd is serving and before any Phase 2 service is planned or
started. [*provision.runs-after-registryd-and-before-phase-2]

## Entries

Each child key under `Machine\System\Init\ProvisionedPaths\` is one
entry. [*provision.each-child-key-is-an-entry] Unknown values on an entry
are ignored. [*provision.unknown-values-are-ignored]

| Value | Type | Required | Default | Meaning |
|---|---|---|---|---|
| `Kind` | string | yes | — | `directory` or `file`. |
| `Path` | string | yes | — | Absolute path to create or verify. |
| `Security` | binary | no | built-in | The Peios file Security Descriptor to apply. |
| `Required` | dword | no | 0 | If 1, failing this entry prevents Phase 2. |

For `Kind=directory` peinit ensures the path exists as a directory; for
`Kind=file` it ensures the path exists as a regular file.
[*provision.kind-decides-what-is-ensured] In either case a path that
exists with a different file type fails the entry.
[*provision.a-type-mismatch-fails-the-entry] An existing file is opened
rather than created, so provisioning never truncates one.
[*provision.an-existing-file-is-never-truncated]

peinit does not create parent directories. The parent is checked, and has
to already be a directory. [*provision.parents-are-not-created] A package
that needs a hierarchy declares
each directory explicitly, or depends on the package that owns the
parent — which keeps the ownership of every directory traceable to a
package rather than to whichever entry happened to run first.

## Descriptors

When `Security` is present, peinit applies the supplied binary
descriptor. [*provision.a-supplied-descriptor-is-applied] A malformed or
rejected descriptor fails the entry.
[*provision.a-bad-descriptor-fails-the-entry]

When it is absent, peinit applies a built-in default
[*provision.the-built-in-default-descriptor]:

```text
O:SY G:SY D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;FR;;;BU)
```

SYSTEM and Administrators get full control; ordinary users get
`FILE_GENERIC_READ`.

## Failure

Entries with `Required=0` are fail-soft: peinit logs and continues.
[*provision.an-optional-entry-is-fail-soft] Entries with `Required=1` are
fail-closed: peinit logs and enters recovery before Phase 2 starts.
[*provision.a-required-entry-that-fails-is-recovery]

An entry that is malformed — a missing or unrecognized `Kind`, a missing
or relative `Path`, a value of the wrong type — is logged as a warning
and skipped, regardless of `Required`.
[*provision.a-malformed-entry-is-skipped-whatever-required-says]
`Required` marks a path as
essential to boot; it does not make a broken entry more dangerous than a
missing one.
