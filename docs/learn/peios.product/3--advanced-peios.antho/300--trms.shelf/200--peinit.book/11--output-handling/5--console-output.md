---
title: Console Output
description: peinit writes its own operational messages to the console and never a service's output — the line format, the stage banner, severity, and the quiet setting.
---

peinit writes its own operational messages to `/dev/console`
[*console.peinit-writes-its-own-messages-to-the-console]:

- Phase 1 progress — mount results, registryd starting.
- Phase 2 progress — services starting and failing, dependency errors.
- Shutdown progress.
- Recovery mode entry.
- Critical service failures.

Service output is never echoed to the console.
[*console.service-output-is-never-echoed] The console is for
peinit's own messages; a service that wants a terminal asks for one with
`TTYPath`.

## The line format [*console.a-line-is-a-tag-then-a-component-then-a-message]

Every line carries a fixed-width outcome tag, then the component name,
then the message:

```text
[      ] peinit: phase1 starting
[  OK  ] peinit: service authd started
[ SKIP ] peinit: service resolvd skipped: DependencyFailure
[ WARN ] peinit warning: calendar timer not armed: Malformed
[FAILED] peinit: service netd failed to launch: token materialisation failed
[ CRIT ] peinit: entering recovery: BootAttempts(3)
```

The tag column is the point of the format. An operator scanning a boot
reads down one column rather than reading sentences, and a failure stops
looking exactly like the twenty lines around it.

| Tag | Means |
|---|---|
| `[      ]` | Progress with no outcome yet. Holds the column while receding. [*console.the-blank-tag-is-progress-with-no-outcome] |
| `[  OK  ]` | It worked. [*console.the-ok-tag-means-it-worked] |
| `[ SKIP ]` | Deliberately not done. [*console.the-skip-tag-means-deliberately-not-done] |
| `[ WARN ]` | Wrong, but the boot continues. [*console.the-warn-tag-means-the-boot-continues] |
| `[FAILED]` | It did not work. [*console.the-failed-tag-means-it-did-not-work] |
| `[ CRIT ]` | The machine is about to be lost. [*console.the-crit-tag-means-the-machine-is-about-to-be-lost] |

An `[  OK  ]` line reads "mounted", never "mounting": the tag already
claims the outcome, so the verb has to agree with it.

### Tag and severity are different axes

The tag is *presentation*. [Severity](#severity-and-quiet) decides who is
allowed to print. They correlate but neither determines the other — a
Normal service failing and a Critical service failing both render
`[FAILED]` while carrying different severities — and collapsing them
would quietly change what `peios.quiet=2` suppresses.

<!-- lint-disable-next-line words spelling -->
### Color [*console.tags-are-coloured-with-sgr-escapes]

Tags are colored with ANSI SGR escapes: green for `OK`, yellow for
`SKIP` and `WARN`, red for `FAILED`, and white-on-red for `CRIT`.
Untagged progress is never colored.

Color is decided once, from the kernel command line, following the same
rule systemd uses: **on unless the command line says `TERM=dumb`.**
<!-- lint-disable-next-line words spelling -->
[*console.colour-is-on-unless-the-command-line-says-term-dumb] peinit
is PID 1 and has no inherited `TERM` to consult, so the command line is
the only place an operator can say a console cannot render escapes.
Deliberately not a `peios.*` token — that set is kept small on purpose,
and `TERM=dumb` is a spelling people already know.

Color changes the bytes and never the layout, so a serial log and a
virtual terminal agree about where the message starts.
<!-- lint-disable-next-line words spelling -->
[*console.colour-never-changes-the-layout]

### The stage banner [*console.a-banner-precedes-phase-1]

Before Phase 1 does any work, peinit prints a banner naming the stage and
the boot mode:

```text
  ══════════════════════════════════════════════════════════════
   peinit · real root · PID 1 · Full boot                 v0.0.1
  ══════════════════════════════════════════════════════════════
```

It is punctuation rather than decoration.
[prelude](~peios/peinit/boot/the-initramfs-contract)
prints the matching one when the initramfs takes over, so the two mark the
handover from kernel to initramfs to real root — the one thing a boot log
otherwise never says out loud.

The mode is the one this boot **starts** in: `Full boot`, `Safe mode`, or
`RECOVERY MODE`. [*console.the-banner-names-the-mode-the-boot-starts-in]
A later downgrade to Safe is announced by its own message
rather than by reprinting a banner, because a second banner would read as
a second stage. [*console.a-later-downgrade-does-not-reprint-the-banner]

Naming the mode is why the kernel command line is read at the very top of
`run_init`, before any Phase 1 work.
[*console.the-command-line-is-read-before-any-phase-1-work] That reverses an
earlier ordering
which assumed mounting the virtual filesystems is what makes
`/proc/cmdline` readable. It is not: the mount step reads
`/proc/self/mountinfo` first, and `/proc` is provided by the initramfs. One
consequence — a machine with both an unreadable command line and another
Phase 1 fault now reports the command line as its recovery reason.

The banner carries ordinary status severity, so `peios.quiet=2` drops it
along with every other kind of progress.
[*console.the-banner-carries-status-severity]

### Who else writes to this console

The format is shared by **specification**, not by shared code. peinit,
[prelude](~peios/peinit/boot/the-initramfs-contract) and the
hook
scripts
each
implement it separately, because prelude is a size-critical initramfs PID 1
with no dependencies and the hooks are shell. Changing a tag word or a
width means changing it in all three.

Everything peinit runs that reaches the console is in this format,
including output peinit did not write itself. **Autorun scripts are
captured**, not left to inherit peinit's streams, and their lines are
relayed under the script's own file name
[*console.autorun-output-is-relayed-under-the-scripts-file-name]:

```text
[      ] 10-apply-seeds.sh: applied 20 file(s), 92 key(s); 0 failed
[  OK  ] peinit: ran 1 autorun script(s)
```

Relayed lines always carry the blank tag.
[*console.a-relayed-line-carries-the-blank-tag] peinit does not know whether a
given line of somebody else's output is good news — the producer knows
and peinit does not, and guessing from prose is how a rename silently
turns an error green. The *producer's* outcome is reported separately,
from its exit code, which peinit does know.

Relayed lines are sanitised first. A script sharing the console could
otherwise steer it with an escape sequence, and peinit's own output
shares the device. Three rules apply:

- Control characters become `?`.
  [*console.a-control-character-in-a-relayed-line-becomes-a-question-mark]
- A line is cut at 512 characters.
  [*console.a-relayed-line-is-cut-at-512-characters]
- A producer is capped at 200 lines, with the remainder counted.
  [*console.a-producer-is-capped-at-200-relayed-lines]

The only lines on a normal boot that are **not** in this format come from
outside peinit's reach entirely: the firmware's own messages before any
kernel runs, and a service that owns the terminal through `TTYPath`
writing to it directly.
[*console.only-firmware-and-a-terminal-owner-write-outside-the-format]

## Severity and quiet

Each message carries a severity, and `peios.quiet` (§2.6) decides what
that means:

- At `0`, everything is written.
- At `1`, the default, peinit stays out of a terminal held as the
  controlling terminal of a running service, except to announce loss of
  the system. Terminals are matched by device rather than by path, since
  `/dev/console` and `/dev/ttyS<n>` can be the same device; where the
  device cannot be determined peinit assumes the terminal is held.
- At `2`, ordinary progress is dropped everywhere while errors still
  get through.

Suppressed messages are discarded rather than buffered for later.

Shutdown progress carries ordinary status severity, so `peios.quiet=2`
suppresses it along with every other kind of progress.
[*console.shutdown-progress-carries-status-severity]

The autorun step in Phase 1 (§2.3) bypasses the policy entirely, on the
grounds that a script running that early and going wrong is worth
interrupting anything for. Its lines are still tagged.

## Phase 1 registryd failures

One deliberate exception to "service output is never echoed to the
console". peinit gives every service capture pipes and drains them in the
runtime loop — but Phase 1 has no loop, so a registryd that printed
exactly why it could not serve was reported to the operator as nothing but
`registryd readiness timeout expired before READY=1`, with the reason
sitting unread in a pipe.

When Phase 1 registryd fails to become ready, or says `READY=1` and then
cannot serve, peinit now drains those pipes and relays what it found
[*console.a-phase-1-registryd-failure-is-relayed-to-the-console]:

```text
[FAILED] peinit: registryd failed; what it said follows
[      ] registryd(stderr): open hive Machine: permission denied
```

Failure path only. On a normal boot the descriptors are retained for the
runtime, which drains them into the pre-eventd buffer and on to eventd
like any other service's output — so registryd's startup lines are
queryable with `evctl` and are **not** on the console.
[*console.on-a-normal-boot-registryds-output-goes-to-eventd-alone]

The drain is safe to run from PID 1 because the read ends are
non-blocking: a registryd that is alive and silent yields `EAGAIN` rather
than hanging the boot.

> [!NOTE]
> `peios.quiet` governs peinit only. What the **kernel** prints is
> governed separately by `loglevel`, which shipped images set to `4`. See
> [Boot and boot modes](~peios/services-and-jobs/boot-and-boot-modes).
