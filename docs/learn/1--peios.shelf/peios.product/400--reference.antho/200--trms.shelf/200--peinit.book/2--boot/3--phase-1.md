---
title: Phase 1
description: The compiled-in phase with no registry dependency, whose whole purpose is to reach the point where a registry can serve.
---

Phase 1 is compiled into peinit. It does not change at runtime and has
no registry dependency, because its whole purpose is to reach the point
where a registry exists. It does the minimum needed to make Phase 2
possible, and most of its failures are fatal to the boot.

## Step 1: Confirm the root is writable [*phase1.root-writability-is-probed]

The initramfs delivers the root mounted read-write. peinit does not
remount it — mount flags belong to the initramfs, and a redundant
remount of an already-writable or overlay root can fail for reasons that
have nothing to do with the root being usable.

Instead peinit probes. It creates `/.peinit/` if it is absent, writes a
uniquely named file there — the name is derived from peinit's own PID
and a namespace identifier, so two probes cannot collide — writes to it,
and removes it. If any part of that fails, the root is not usable for
Phase 2 and peinit enters recovery mode.
[*phase1.an-unwritable-root-is-recovery]

## Step 2: Mount what is missing [*phase1.mounts-only-what-is-absent]

`/proc`, `/sys`, and `/dev` are already mounted. peinit does not
blindly mount them again: a redundant mount stacks a second filesystem
over the populated one, and on some kernel and flag combinations returns
`EBUSY` instead.

peinit reads `/proc/self/mountinfo` to find out what is already there,
and mounts only what is not:

| Mount point | Filesystem | Flags | Provided by |
|---|---|---|---|
| `/proc` | proc | nosuid, nodev, noexec | initramfs |
| `/sys` | sysfs | nosuid, nodev, noexec | initramfs |
| `/dev` | devtmpfs | nosuid | initramfs |
| `/dev/pts` | devpts | nosuid, noexec | peinit |
| `/dev/shm` | tmpfs | nosuid, nodev | peinit |
| `/run` | tmpfs | nosuid, nodev | peinit |
| `/sys/fs/cgroup` | cgroup2 | nosuid, nodev, noexec | peinit |
| `/sys/kernel/security` | securityfs | nosuid, nodev, noexec | peinit |

Each `mount(2)` passes the filesystem name as both the source and the
filesystem type, passes only the listed flags, and passes null mount
data. Mount points that do not exist are created first.

`/dev/pts` additionally gets a KACS mount policy immediately after it is
mounted [*phase1.devpts-gets-a-synthesise-ephemeral-policy]:
`SYNTHESIZE_EPHEMERAL`, with a template descriptor granting
SYSTEM and Administrators full control and Authenticated Users read,
write and traverse. devpts cannot store security descriptors, and its
terminal endpoints are materialised by the kernel when a terminal opens
`/dev/ptmx` — never through a create path that could stamp or inherit
one — so under the default `DENY_MISSING` every pseudo-terminal on the
machine is unopenable. The policy is set through
`SYS_KACS_SET_MOUNT_POLICY` on an `O_PATH` descriptor of the mount
point. The template applies to every SD-less inode of the mount alike,
so any authenticated principal can open any terminal endpoint. Per-owner
endpoint descriptors need kernel support for stamping the opener's identity at
materialisation, and are recorded as future work, not provided here.

`/sys/kernel/security` gets the same kind of policy, for the same
reason: securityfs cannot store descriptors either. Its template grants
SYSTEM full control and Authenticated Users read and traverse
[*phase1.securityfs-gets-a-synthesise-ephemeral-policy]:

```text
O:SYG:SYD:(A;;GA;;;SY)(A;;GRGX;;;AU)
```

KACS publishes `kacs/self` and `kacs/sessions` there (see
[Logon
sessions](~kernel/internals/kacs/tokens/logonsessions-and-revocation)), and
each makes its own check when read, so opening `sessions` is allowed to
anyone signed in while reading it stays with Administrators and SYSTEM.
Only SYSTEM can write: `lockdown` is the one writable file on the mount,
and raising the kernel's lockdown level is not routine administration.

There is a bootstrap wrinkle in reading mountinfo at all: the file lives
in `/proc`, which is one of the things being checked for. If the read
fails with `ENOENT` or `ENOTDIR`, peinit mounts `/proc` from the table
and retries. [*phase1.mountinfo-enoent-retries-after-mounting-proc] Any
other failure to read or parse mountinfo sends peinit to recovery.
[*phase1.unreadable-mountinfo-is-recovery]

For the three initramfs-provided rows, an already-mounted filesystem is
success, and so is an `EBUSY` from an attempted mount.
[*phase1.an-already-mounted-kernel-filesystem-is-success] For the five
peinit owns, a mount failure sends peinit to recovery.
[*phase1.a-failed-peinit-owned-mount-is-recovery]

After successful mount setup, peinit ensures `/run/lock` exists as a
directory. [*phase1.runtime-lock-directory-is-restored] Mounting `/run`
hides the directory shipped by fsbase, so peinit recreates it in the live
filesystem before starting services. A newly created directory inherits
`/run`'s descriptor; an existing directory is preserved. This step also
runs when `/run` was already mounted. A creation failure enters recovery.

<!-- lint-disable-next-line line-length -->
### Seeding descriptors on the new filesystems [*phase1.fresh-mounts-are-seeded]

Three of the five filesystems peinit mounts are fresh and empty:
`/dev/shm`, `/run`, and `/sys/fs/cgroup`. Under KACS an inode with no
Security Descriptor is denied to every caller, and there is nothing on a
newly mounted tmpfs for a new inode to inherit from — so peinit stamps
the mount root with a descriptor that grants SYSTEM and Administrators
full control, inheritable by both containers and objects, and grants
Everyone read and execute, inheritable by containers only:

```text
O:SY G:SY D:(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)(A;CI;GRGX;;;WD)(A;OICIIO;GA;;;S-1-3-0)
```

Everything created underneath — the notify socket, per-service runtime
directories, the cgroup hierarchy — inherits from it; the two sockets
that need a different population are stamped explicitly in step 9. This is why
peinit never sets mode bits on the sockets it creates;
under KACS they would mean nothing, and the descriptor is the thing that
does the work. It is the same descriptor the initramfs seeds onto the
root filesystem but for the inheritance flags on that last ACE, and the
two are kept in step on purpose: this one inheritable ACL is, in
practice, the access policy of everything under these mounts, so an ACE
missing here is missing from every per-service directory under
`/run/services`.

The Everyone ACE grants execute as well as read because on Peios execute
*is* traverse — `FILE_TRAVERSE` and `FILE_EXECUTE` are the same bit — and
an explicit `chdir` is the one traversal that does not get the
`SeChangeNotifyPrivilege` bypass. A service running as anything other
than SYSTEM cannot otherwise reach its own runtime directory. SYSTEM and
Administrators keep `GenericAll` rather than read/write/execute because
`GenericAll` carries `WRITE_DAC`, `WRITE_OWNER` and `DELETE`, and peinit
re-stamps descriptors throughout `/run`.

The last ACE is `CREATOR OWNER`, and it is what makes the container-only
Everyone ACE survivable. A file created under these mounts inherits the
SYSTEM and Administrators ACEs and nothing else — and that is a
*non-empty* DACL, so the creating token's default DACL is never
consulted. Without it a service would create runtime state it could not
read back. It is inherit-only, so it grants nothing on the directory it
sits on; it is resolved against whoever creates each object, and carried
onward down each container.

The Everyone ACE is inheritable by containers only, which is the one
place this descriptor differs from the root filesystem's.
[*phase1.seed-everyone-ace-is-container-inherit-only] What a service needs
here
is to walk to its own directory, and container inheritance gives it that
by itself. Object inheritance would additionally put Everyone-read on
every *file* anything creates beneath these mounts — a service's runtime
state, and shared memory segments, readable by any principal through
inheritance alone. The root filesystem needs object inheritance because a
service has to read and execute the binaries beneath it; nothing under
these three mounts is a binary.

This descriptor is not the one prelude's `seed-sd` writes by default.
That one is narrower, and stays narrower: it also stamps `/dev`, where
every ACE is inherited by the next hot-plugged block device, so a read
ACE for Everyone there would be every filesystem descriptor on the
machine bypassed by opening the raw disk.

Failure to apply the descriptor sends peinit to recovery.
[*phase1.a-failed-seed-is-recovery] Without it every file peinit later
creates on that filesystem would be unreachable to everything, including
peinit.

`/proc`, `/sys`, and `/dev` are not stamped: they arrive from the
initramfs already populated.
[*phase1.the-initramfs-filesystems-are-not-stamped]

### Device node policy [*phase1.device-nodes-get-explicit-descriptors]

`/dev` arrives with that same inheritable descriptor on its root and on
every node, which is the right default — whatever the root grants, a
disk hot-plugged later inherits, so the root must be acceptable on a raw
block device — but it leaves `/dev/null` unusable by anyone who is not
an administrator. A single inherited descriptor cannot say "`/dev/null`
for everyone, the disks for administrators", so peinit enumerates the
exceptions. Once the mounts are up it stamps each of these nodes with a
descriptor of its own:

| Node | DACL |
|---|---|
| `/dev/null`, `/dev/zero`, `/dev/full`, `/dev/random`, `/dev/urandom`, `/dev/tty`, `/dev/ptmx` | `D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;FRFW;;;WD)` |

Everyone can open the node for reading and writing and can stat it;
nobody but SYSTEM and Administrators can change its descriptor. Only the
DACL is replaced — owner and group stay as the seed left them
[*phase1.device-node-stamping-replaces-only-the-dacl] — and the ACEs
carry no inheritance flags, because a device node has no children.
`/dev/console` is deliberately not on the list: it is the SYSTEM
console. [*phase1.console-is-not-in-the-device-node-list]

The step is advisory. A node that cannot be stamped is reported as a
warning and stays on the inherited default, usable by administrators
and denied to everyone else; a node that does not exist is noted and
skipped. Neither sends peinit to recovery.
[*phase1.device-node-stamping-is-advisory]

<!-- lint-disable-next-line line-length -->
## Step 3: Restore the persisted random seed [*phase1.the-random-seed-is-restored]

peinit restores the seed at `/var/state/peinit/random-seed` once `/dev`
is available and before registryd starts. The seed is a machine-local
entropy cache for the kernel CSPRNG. It is not configuration, and
shipping one in a packaged image, live ISO or VM template would hand
every instance of that image the same starting entropy.

If the file is absent, that is an ordinary first boot or a stateless
live boot, and peinit continues silently.
[*phase1.an-absent-random-seed-is-silent] When a seed is present peinit
mixes it into the kernel pool, preferring the interface that credits
entropy for a locally persisted seed; if that fails it mixes the bytes
without crediting and records the failure.
[*phase1.a-seed-that-cannot-be-credited-is-still-mixed] A seed file that
is empty, or larger than 4096 bytes, is treated as an error.
[*phase1.an-empty-or-oversized-seed-is-an-error]

Nothing in this step can send peinit to recovery.
[*phase1.no-seed-failure-enters-recovery] A system with no
entropy cache still boots; it starts with less entropy, which is a
problem for the image builder to solve with a hardware or virtio RNG
rather than with a seed baked into the image.

The initramfs can perform the same restore earlier, once the persistent
root is mounted. peinit's restore stays as the fallback for initramfs
images that do not participate and for boots that have no initramfs.

## Step 4: Ensure the local machine ID [*phase1.the-machine-id-is-ensured]

`/lcl/etc/machine-id` holds a stable local install identifier used for
software compatibility, log correlation and instance identity. It is not
a security principal: not a credential, not a SID, not an account, and
not an input to any authorization decision.

The format is 128 bits as exactly 32 lowercase hexadecimal characters
followed by one newline. [*phase1.machine-id-format] A valid existing
file is left alone. [*phase1.a-valid-machine-id-is-left-alone] A file
that is absent, empty, all zeroes, the wrong length, not hexadecimal, or
missing its trailing newline is replaced
[*phase1.a-malformed-machine-id-is-replaced]: peinit draws 128 bits from
the kernel CSPRNG and writes a valid file atomically, through a temporary
file and a rename, with the result flushed.
[*phase1.a-new-machine-id-is-written-atomically]

The write creates `/lcl/etc/` if it is not there. This step runs before
the path-provisioning machinery of step 7, and `/lcl` is a StrataFS view
the initramfs assembled, so whether the directory exists is a property of
the image rather than something peinit can assume.

Only a **CSPRNG failure** sends peinit to recovery.
[*phase1.a-csprng-failure-is-recovery] If the kernel cannot produce
random bytes, nothing else on the machine can be trusted either.

An unreadable file or an unwritable path does not. The boot continues
with an identifier valid for this boot only, and says so
[*phase1.an-unpersistable-machine-id-is-fail-soft]:

```text
peinit warning: machine-id not persisted (<reason>); using an identifier
for this boot only
```

A read failure takes the same path as finding no file, because it leaves
peinit in the same position — unable to say whether a valid identifier is
on disk. It is reported rather than treated as a clean generation,
because it can have replaced something valid.

This is deliberately fail-soft. The identifier is a local opaque install
ID and not an authorization input, so failing a boot over it would be
disproportionate — and an ephemeral ID is already the defined behavior
for a stateless live boot.

Images and templates are expected to ship with no machine ID, or with an
empty file as a reset marker. Clone tooling that wants a new identity
removes or truncates the file and lets the next boot generate one.
Stateless live boots without a persistent overlay get an ephemeral ID
for that boot.

<!-- lint-disable-next-line line-length -->
## Step 5: Set the clock from the hardware RTC [*phase1.the-clock-is-set-from-the-rtc]

peinit reads the hardware clock and calls `clock_settime()` before
registryd starts, so that timestamps on registry operations, log entries
and the boot attempt counter mean something.

It opens `/dev/rtc`, falling back to `/dev/rtc0` if that device is
absent [*phase1.rtc-falls-back-to-rtc0], and reads it with `RTC_RD_TIME`.
The returned `struct rtc_time` is interpreted as UTC and converted to
`CLOCK_REALTIME` seconds with zero nanoseconds.
[*phase1.the-rtc-is-read-as-utc]

Every failure in this step sends peinit to recovery: no openable RTC
device, a failed read, a value that is invalid or before the Unix epoch,
a failed `clock_settime`, and — deliberately — a failure to close the
descriptor after a successful read. [*phase1.any-rtc-failure-is-recovery]
Leaking a descriptor in PID 1 during
bootstrap is a symptom of something being badly wrong, not a detail to
swallow.

> [!NOTE]
> NTP corrects the clock properly later in the boot. This step only gets
> it into the right decade, so that early timestamps are not absurd and
> the timer subsystem has a wall clock to anchor to.

## Step 6: Start registryd

peinit holds a compiled-in definition for registryd — the only compiled-in
service definition there is
[*phase1.registryd-is-the-only-compiled-in-definition]:

| Field | Value |
|---|---|
| ImagePath | `/sbin/registryd` |
| Arguments | `Machine=/var/state/loregd/Machine.hive`, `Users=/var/state/loregd/Users.hive` |
| Identity | `SYSTEM` |
| Readiness | Notify |
| ErrorControl | Critical |

The hive paths are peinit's choice, not registryd's: where the machine
registry lives is a boot-policy decision, and it is made here because
this is the one service start that cannot consult configuration.

peinit mints a SYSTEM token including registryd's per-service SID,
creates the cgroup tree, forks with the token installed, and execs
`/sbin/registryd` through the runtime StrataFS view.
[*phase1.registryd-is-started-like-any-service] Two separate timeouts
bound the start, both 30 seconds: one on process setup, driven
synchronously because there is no event loop yet, and one on readiness.
[*phase1.registryd-start-has-two-thirty-second-timeouts]

registryd's `READY=1` means "accepting and serving registry requests",
not "the process is alive" — it does not signal until its storage
backend is open, its schema is validated and it can answer a read.
[*phase1.registryd-readiness-means-serving]

### The schema-version guard [*phase1.the-schema-version-guard]

After readiness, peinit ensures the base registry structure exists and
then probes it. Ensuring comes first: peinit creates `Machine\System`,
`Machine\System\Services` and `Machine\System\Init` if they are absent
and stamps `Machine\System\Services\SchemaVersion` with the current
schema version, 1. Only then does it read the value back.
[*phase1.the-base-registry-structure-is-ensured-then-probed]

The read is what verifies registryd is genuinely serving. A value that
is present but not a `REG_DWORD`, or that is a `REG_DWORD` of the wrong
length, fails the probe. [*phase1.a-malformed-schema-version-fails-the-probe]
A key or value that is absent reads as zero and passes
[*phase1.an-absent-schema-version-passes] — the structure was
created, so absence at this point means the write did not take effect,
and the failure that matters is the provisioning failure, which is
reported directly.

The consequence is that the guard is self-healing. An unprovisioned
first boot, or a registry cleared by the recovery tools, comes up with
an empty `Machine\System\Services\` and boots into a Phase 2 with no
services rather than into recovery.
[*phase1.an-empty-service-key-boots-with-no-services]

### Keeping registryd [*phase1.registryd-is-retained-as-a-runtime-instance]

When registryd passes readiness and the probe, peinit retains the
activation as an ordinary runtime service instance: its state, its
pidfd-tracked main process, its cgroup generation and paths, its output
pipe ownership, its notify generation, its job identity, and any cleanup
evidence. Ownership is not dropped at the Phase 2 boundary.

During Phase 2 the registry's own definition of `registryd`, if there is
one, is merged onto the retained activation. peinit does not create a
second inactive record and does not restart registryd because a
definition has appeared.
[*phase1.a-registry-definition-of-registryd-is-merged-not-restarted] If
the registry definition is absent or invalid, the ordinary graph
validation rules apply.

If registryd fails to start, its readiness times out, or the probe
fails, peinit enters recovery. There is no Phase 2 without a registry.
[*phase1.no-registryd-is-recovery]

<!-- lint-disable-next-line line-length -->
## Step 7: Autorun scripts [*phase1.autorun-scripts-run-between-registryd-and-provisioning]

Between registryd starting and path provisioning, peinit runs every
non-directory entry in `/lcl/policy/autorun.d`, in sorted order, by
absolute path [*phase1.autorun-runs-every-entry-in-sorted-order], with
the working directory `/` and `PATH=/sbin:/bin`
[*phase1.an-autorun-scripts-environment]. Each runs under peinit's own
SYSTEM token. [*phase1.autorun-scripts-run-as-system]

The step is fail-open at every point: a missing directory, an unreadable
directory, a spawn failure, and a non-zero exit are all console warnings
and none of them stops the boot. [*phase1.autorun-is-fail-open] Its
console output bypasses the quiet policy (§2.6), because a script that
ran this early and went wrong needs to be visible.
[*phase1.autorun-output-bypasses-the-quiet-policy]

The child's standard output and standard error share one pipe. peinit drains
that pipe before waiting for the child, so a script can write more than pipe
capacity to either stream without blocking behind an unread second pipe.
Writes reach the captured stream in pipe order; the child's own buffering
still controls when those writes happen. If reading fails, peinit attempts
to terminate and reap the child before reporting the failure.

## Step 8: Provision boot-time paths

Covered in §2.4.

## Step 9: Infrastructure setup

Three things, before Phase 2 begins:

1. **The control socket** at `/run/services/peinit/control.sock`, which
   serves every runtime command for the lifetime of the system, stamped
   so that any authenticated principal can connect and every command is
   then checked on its own (§10.1).
   [*phase1.the-control-socket-is-created]
2. **The jobs socket** at `/run/services/peinit/jobs.sock`, on which
   any authenticated principal can submit a job once Phase 2 runs,
   stamped so that connecting is the submission permission (§10.7).
   [*phase1.the-jobs-socket-is-created]
3. **Loopback**: peinit brings up `lo` over netlink, because services
   that bind `127.0.0.1` need it. [*phase1.loopback-is-brought-up]

Either socket failing to bind or to take its descriptor sends peinit to
recovery [*phase1.a-socket-failure-is-recovery] — without the first there
is no way to administer the system, and a jobs socket with the wrong
descriptor is either unreachable or open to everything. A loopback
bring-up failure is a warning that lets Phase 2 proceed.
[*phase1.a-loopback-failure-is-a-warning]

## Failure summary

| Failure | Response |
|---|---|
| Root writability probe fails | Recovery |
| A mount point cannot be created | Recovery |
| A peinit-owned filesystem fails to mount | Recovery |
| A mounted filesystem cannot be stamped with its descriptor | Recovery |
| `/proc/self/mountinfo` unreadable or unparseable | Recovery |
| `/proc`, `/sys` or `/dev` already mounted, or `EBUSY` | Tolerated as success |
| A device node in the policy list cannot be stamped | Warning; node keeps the inherited default |
| A device node in the policy list does not exist | Noted; boot continues |
| Random seed absent, oversized, empty, or unrestorable | Warning; boot continues |
| Machine ID generation fails (CSPRNG) | Recovery |
| Machine ID unreadable, or unwritable at its path | Warning; boot continues with an identifier for this boot only |
| Machine ID absent, empty, or malformed | Regenerated; boot continues |
| Any RTC or clock failure | Recovery |
| registryd fails to start, or setup times out | Recovery |
| registryd readiness times out | Recovery |
| Base registry provisioning fails | Recovery |
| Schema-version probe returns a wrong type or length | Recovery |
| An autorun script is missing, unspawnable, or exits non-zero | Warning; boot continues |
| A provisioning entry is malformed | Warning; entry skipped |
| An optional provisioned path fails | Warning; boot continues |
| A required provisioned path fails | Recovery |
| Control socket creation or stamping fails | Recovery |
| Jobs socket creation or stamping fails | Recovery |
| Loopback bring-up fails | Warning; boot continues |
