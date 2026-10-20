---
title: Per-Service SIDs
description: The SID derived from a service's name and carried in its token's group list — why it exists, and the uppercasing rule.
---

Every service token carries a SID derived from the service's name, in
its group list, alongside whatever the principal's own identity brings.
[*sid.every-service-token-carries-one]

The derivation is the one KACS defines (Peios Kernel TRM §3.2). The
authority is `S-1-5-80`. The service name is uppercased and encoded as
UTF-16LE, SHA-1 is taken over those bytes, and the 20-byte digest is
split into five little-endian 32-bit sub-authorities:
[*sid.the-derivation-is-sha1-over-the-uppercased-name]

```text
S-1-5-80-<sub1>-<sub2>-<sub3>-<sub4>-<sub5>
```

peinit computes this itself, from the name alone, with no involvement
from anything else. authd computes the same value independently when it
mints a token, and the two implementations are pinned against each other
by a shared test vector. [*sid.peinit-and-authd-derive-the-same-sid]

## Why they exist

Per-service SIDs are what make access control useful when services share
a principal. Every platform daemon runs as SYSTEM, and every service
with no `Identity` runs as `LocalService`; without something to tell
them apart, an ACL could grant a right to "LocalService" and thereby
grant it to a dozen unrelated services.

With a per-service SID, an ACE can name one specific service. It costs
nothing — no account, no registry entry, no allocation — because it is a
hash of a name that already has to be unique.

They are load-bearing in more than access checks. peinit uses the
service SID directly when it provisions a service's runtime directories,
stamping `/run/<name>` with a descriptor that grants full access to
SYSTEM, Administrators, and that service's SID and nothing else (§3.3).

## The uppercasing rule [*sid.the-name-is-uppercased-before-encoding]

The name is uppercased before encoding, using full Unicode case mapping
— the one that expands `ß` to `SS` and `ﬁ` to `FI` rather than mapping
each code unit in place. peinit uppercases in the code-point domain,
before the UTF-16 encoding, so any expansion happens first.

For an ASCII service name, which is every real service, the choice is
invisible. It becomes visible only for a name containing a character
whose uppercase form is longer than itself.
