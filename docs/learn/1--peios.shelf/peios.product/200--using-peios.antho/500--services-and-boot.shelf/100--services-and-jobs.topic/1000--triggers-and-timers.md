---
title: Triggers and Timers
type: concept
description: Choose automatic starts, inspect the next timer run, disable triggers safely, and configure calendar schedules and catch-up.
related:
  - peios/services-and-jobs/defining-a-service
  - peios/services-and-jobs/service-types
  - peios/services-and-jobs/the-service-lifecycle
  - peios/services-and-jobs/jobs-and-operations
  - peios/services-and-jobs/controlling-services
---

To see when a scheduled service will run, use `svctl status <service>` or the
**Next run** column in Services Manager. To change automatic starts, edit the
definition's `Triggers` field. A saved schedule is not evidence that a timer
is armed; check status after saving it.

A **trigger** decides when the service starts on its own. It is independent of
[service type](~peios/services-and-jobs/service-types): boot and timer
triggers can start either Simple or Oneshot services. Each entry in `Triggers`
is written `type` or `type:argument`.

| Trigger | Form | Meaning |
|---|---|---|
| Boot | `boot` | Start during Phase 2, in dependency order. |
| Settled boot | `boot:settled` | Start after the boot set settles, with a bounded wait; useful for console prompts. |
| Terminal handover | `tty:released` | Start when the terminal named in `TTYPath` is released. |
| Timer | `timer:<schedule>` | Start on a [calendar schedule](#calendar-expressions). |

Without triggers, the service is **demand-only**: it starts through a control
command or because another service needs it. To prevent automatic activation
while keeping manual start, use [Disabled](#disabled).

## Seeing when a timer runs

peinit reports each timer as it has it armed, in the service's `status`:
- the schedule's next occurrence;
- when the timer will actually fire, with its jitter added;
- when it last fired.

A schedule peinit refused, such as `*-02-30`, which never comes round, is
reported with the reason, rather than left out.

```text
$ svctl status logrotate
...
timers:
  *-*-* 02:00:00
    next: 2026-06-02T02:00:00Z, firing at 2026-06-02T02:07:12Z after jitter
    last fired: 2026-06-01T02:03:40Z
```

`svctl list` has a **NEXT TIMER** column, with each service's soonest firing.
On the desktop, Services Manager shows the same things in local time: a **Next
run** column, and each timer in the details pane. See [Controlling
services](~peios/services-and-jobs/controlling-services#reading-status) for
the fields.

These times are peinit's own. Jitter is drawn at random each time a timer is
armed, so the time a timer fires can't be worked out from its definition.

## Disabled

`Disabled=1` suppresses **automatic activation**. A disabled service will not
be started by *any* trigger — boot, timer, terminal handover, or anything
added later — but it can still be started by hand through the control
interface. It is the switch for "configured, but not running on its own right
now."

Two related controls are easy to confuse with it:

| To… | Use |
|---|---|
| Stop a service from auto-starting, but keep manual start | `Disabled=1` |
| Stop a service from being started *at all*, even manually | Deny `SERVICE_START` in its [ServiceSecurity](~peios/services-and-jobs/who-can-manage-a-service) descriptor |
| Stop a *running* service | The [`stop` command](~peios/services-and-jobs/controlling-services) |

Enabling and disabling are registry writes performed by admin tooling, not
peinit commands. peinit picks up a `Disabled` change through a registry
notification or on `reload-config`. A
disabled service's definition is still loaded into peinit's model, so it is
ready for an on-demand start the instant you ask.

## Calendar expressions

Timer schedules are written in systemd's `OnCalendar` format. peinit's grammar
is normative and self-contained — what follows is the whole language. The
general shape is:

```text
DayOfWeek Year-Month-Day Hour:Minute:Second Timezone
```

Every field is optional and has a default, so most real schedules are short.

| Field | Form | If omitted |
|---|---|---|
| DayOfWeek | weekday name(s) | any day (`*`) |
| Date | `Year-Month-Day` | `*-*-*` (any date) |
| Time | `Hour:Minute:Second` | `00:00:00` |
| Second | the `:Second` of Time | `:00` |
| Timezone | IANA zone name | system-local time |

So `02:00:00` is a complete expression — date defaults to *every day*, and it
means "every day at 02:00." `Hour:Minute` with no seconds defaults the seconds
to `00`.

### Component syntax

Each numeric component — year, month, day, hour, minute, second — and the
weekday accept the same operators:

| Operator | Example | Matches |
|---|---|---|
| **Wildcard** | `*` | any value |
| **List** | `1,15` | any listed value |
| **Range** | `Mon..Fri`, `8..17` | the inclusive range |
| **Repetition** | `0/15` | `0`, then every multiple of the step — 0, 15, 30, 45 |
| **Range + step** | `8..17/2` | `8`, `10`, `12`, … up to and including `17` |

Weekdays are English names, case-insensitive, abbreviated (`Mon`) or full
(`Monday`), and take lists and ranges.

### Last day of the month

<!-- "May" here is the month. -->
<!-- lint-disable spelling -->
A `~` in place of the `-` between Month and Day counts the day **from the
end** of the month: `~01` is the last day, `~02` the second-to-last, and so
on. So `*-*~01` is the last day of every month. Repetition combines with it —
`Mon *-05~07/1` is "the last Monday in May."
<!-- lint-enable spelling -->

### Named shortcuts

These stand in for the expression shown:

| Shortcut | Equivalent |
|---|---|
| `minutely` | `*-*-* *:*:00` |
| `hourly` | `*-*-* *:00:00` |
| `daily` | `*-*-* 00:00:00` |
| `weekly` | `Mon *-*-* 00:00:00` |
| `monthly` | `*-*-01 00:00:00` |
| `quarterly` | `*-01,04,07,10-01 00:00:00` |
| `semiannually` | `*-01,07-01 00:00:00` |
| `yearly` / `annually` | `*-01-01 00:00:00` |

### Timezones and precision

A timezone specifier follows the IANA database — `Europe/London`,
`US/Eastern`, `UTC`. With none, the expression is interpreted in
system-local time.

Precision is **second-level**. Unlike systemd, peinit does not accept
sub-second fractions; a fractional second is a parse error. Service scheduling
has no use for finer granularity.

### Examples

| Expression | Meaning |
|---|---|
| `*-*-* 02:00:00` | Every day at 2 am (system-local) |
| `Mon *-*-* 00:00:00` | Every Monday at midnight |
| `*-*-1,15 12:00:00` | 1st and 15th of each month at noon |
| `*-*~01 00:00:00` | Last day of each month at midnight |
| `Mon..Fri *-*-* 09:00:00` | Weekdays at 9 am |
| `*-*-* *:00/15:00` | Every 15 minutes |
| `*-*-* 02:00:00 Europe/London` | Every day at 2 am London time |

## What happens when a timer fires

A timer firing does not blindly start the service — peinit considers the
service's current state first, so a firing never collides with a run already
in progress.

| Type | Current state | Action |
|---|---|---|
| Oneshot | Inactive / Completed / Failed | Start it (operation source `Timer`). |
| Oneshot | Active / Starting | Set a single **pending** flag — one catch-up run when the current one finishes. |
| Simple | Inactive / Failed | Start it. |
| Simple | Active / Starting | No-op; the firing is logged. |

The Oneshot "pending" flag is deliberately not a queue: many firings that land
while a Oneshot is busy collapse into *one* catch-up run, taken when the
current run ends. A service cannot stampede itself.

That single flag is shared across *all* of a Oneshot's timers, not one per
timer. If a Oneshot has several timer triggers and two of them fire while it
is busy, at most **one** run is still pending when the current one finishes.
The timers are otherwise fully independent — each keeps its own next-firing
time and its own last-run timestamp — but they can only ever queue up a single
shared catch-up between them.

Each timer firing creates a normal [start
operation](~peios/services-and-jobs/jobs-and-operations), so a timer-started
service is observable exactly like an admin-started one — same GUIDs, same
events.

## Persistent timers and catch-up

`TimerPersistent` (default `1`) controls whether runs **missed while the
machine was off** are caught up after a reboot.

peinit records the last-run timestamp of each timer in the registry. On boot,
for each persistent timer it reads that timestamp, computes the next firing
after it, and — if that time is already in the past — **fires once,
immediately**, then resumes the normal schedule.

The catch-up is always a *single* run, never one-per-missed-occurrence. A
daily timer that was off for five days fires once on the next boot, not five
times. A non-persistent timer (`TimerPersistent=0`) ignores history entirely
and computes its next future firing from now.

One subtlety worth knowing: the last-run timestamp is keyed by the **schedule
string**. If you change a timer's schedule, the old timestamp is orphaned and
the new schedule has no history, which causes exactly one spurious catch-up
run. Schedule changes are rare and the cost is one extra run, so peinit
accepts it rather than tracking trigger identity.

> [!NOTE]
> The timestamp is written *when the timer fires and the start is initiated*,
> not when the run completes. A service that crashes mid-run is not
> re-triggered on the next boot — the run was attempted, not missed.

## Jitter

`TimerJitter` (default `0`) adds a random delay of 0 to `TimerJitter` seconds
to each firing, recomputed every time. A timer scheduled for midnight with
`TimerJitter=900` fires at a slightly different moment between 00:00 and 00:15
each day. Jitter only ever delays — a timer never fires *before* its scheduled
time. It is the standard remedy for a fleet of machines all hammering the same
backend at exactly midnight.

## When the clock jumps

Calendar timers are **wall-clock** schedules: `*-*-* 02:00:00` means "02:00 on
the wall," not "every 86,400 seconds." peinit arms them as absolute real-time
timers that are canceled and recomputed whenever the system clock is stepped,
so the schedule stays anchored to the wall clock across corrections.

| Event | Behavior |
|---|---|
| **NTP step or manual clock set** | Armed calendar timers are canceled and recomputed against the new wall clock. A backward step pushes the next firing later; a forward step that crosses a missed time fires it once. |
| **Suspend / resume** | A scheduled time that elapsed while suspended fires once on resume — once, not once per missed occurrence. |
| **A missed occurrence within one uptime** | Fire once, then resume the normal schedule. peinit never replays every occurrence in the gap. |
| **Daylight-saving transition** (timezone-aware timer only) | A scheduled time in the *skipped* hour never fires; a scheduled time in the *repeated* hour fires once. See below. |

Daylight-saving is the case most likely to surprise you, and it only affects
timers that name an IANA zone that observes DST (a bare `02:30:00` in
system-local time follows whatever the system does). For a scheduled local
time inside the affected hour:

- **Spring forward** — that time does not exist on the transition day. The
  timer does not fire for that occurrence; it resumes at the next valid
  occurrence.
- **Fall back** — that time occurs twice. The timer fires exactly once, on the
  first occurrence.

This is distinct from peinit's *interval* timers — the watchdog, health-check
intervals, restart backoff, and the start/stop/reload phase timeouts. Those
are genuine durations measured on the monotonic clock, so "wait 30 seconds"
means 30 elapsed seconds and never lurches when the wall clock is set.

There is one boot-time case worth calling out on its own. When peinit reads a
timer's stored last-run timestamp at boot and finds it is *in the future*
relative to the current wall clock — meaning the clock has gone backwards
since that run — it can't trust the timestamp, so it treats the timer as
having an **unknown last run** and fires the persistent catch-up immediately.
This only applies to the boot-time check; the runtime handling above is
unaffected.

> [!CAUTION]
> If a machine boots with a wildly wrong clock (a dead RTC battery) and NTP
> corrects it only later, the boot-time persistent catch-up decision is made
> against the wrong time and can fire spuriously or not at all. The *runtime*
> half self-heals once NTP steps the clock, but the boot-time decision is
> already made by then. This is a known edge case short of NTP-aware
> rescheduling — worth knowing if you run timers on hardware with unreliable
> clocks.

## Waiting for the boot to settle

`boot:settled` is `boot` with one difference: the service starts once the
Phase 2 boot set has stopped moving — every service in it reached a state it
will not leave on its own — rather than during it.

It exists for services that write to a terminal. peinit reports its progress
to `/dev/console`, and a service holding that same terminal writes there too,
so a prompt started mid-boot gets written over: `login` emits `Username: `
with no newline and waits, peinit appends a service line, and the result is
unreadable until you press Enter and the `login` program redraws.

The wait is capped (`Machine\System\Boot\SettleTimeout`, 5 seconds by
default). A service stuck in Starting therefore costs a deferred service a
short delay rather than preventing it — which matters most exactly when
something is broken and a console prompt is what you need.

A `boot:settled` service is deliberately **not** part of the boot plan. It
does not consume the [parallel-start
budget](~peios/boot/boot-and-boot-modes), is not counted toward
boot success, and cannot block another service. Waiting for quiet is a
preference; it must not change what the boot means.

> [!NOTE]
> Reach for this only to keep console output legible. If a service genuinely
> needs something to be running first, that is a
> [dependency](~peios/services-and-jobs/dependencies) — say so with `Requires`
> . A console sign-in service uses both: `boot:settled` for *when*, and
> `Requires` on
> its authority for *what must be true*.

## Queueing for a terminal

A terminal has one owner at a time. Two services writing to the same `TTYPath`
do not fail — they interleave, which on a console means a sign-in prompt with
a
progress line through the middle of it. So peinit gives the device to one of
them and skips the rest, and `tty:released` is how the ones that were skipped
get their turn.

The whole arrangement is three fields. A first-boot setup flow and a sign-in
prompt, both on the console:

| Service | `TTYPath` | `TTYPrecedence` | `Triggers` |
|---|---|---|---|
| `oobe` | `/dev/console` | 100 | `boot:settled` |
| `login-console` | `/dev/console` | *(0)* | `boot:settled`, `tty:released` |

On a first boot both are deferred, `oobe` outranks the sign-in prompt on
`TTYPrecedence`, so `oobe` takes the console and `login-console` is
**skipped** with cause `TtyUnavailable`. When `oobe` finishes — or crashes,
or is stopped; all three free the device — `tty:released` starts the sign-in
prompt. On every later boot `oobe` is gone, nothing is holding the console,
and the sign-in prompt starts straight away.

A few rules worth knowing:

- **Higher `TTYPrecedence` wins**, and equal precedence breaks on service
  name, so the outcome is the same on every boot. The default is 0.
- **Nobody is preempted.** Precedence decides between services starting at the
  same moment; a service already using a terminal keeps it.
- **A service that stops is not offered its own terminal back.** Relaunching
  it is `RestartPolicy`'s job, not this.
- **Both fields need a `TTYPath`.** `tty:released` or `TTYPrecedence` without
  one is rejected when the definition is read, rather than quietly doing
  nothing.
- **`tty:released` on its own starts nothing** until somebody takes the
  terminal and lets it go. Give a service that must come up either way a
  `boot` or `boot:settled` trigger as well, as `login-console` has above —
  otherwise a machine with no `oobe` installed never gets a sign-in prompt.

Being skipped is not a failure: a skipped service satisfies anything that
depends on it, exactly as a service skipped by a `Conditions` entry does.

> [!NOTE]
> This is deliberately not `Conflicts`, which fails **both** services — and
> the services that want a terminal are the interactive ones, so that answer
> is a machine you cannot sign in to. It is not a dependency either: a sign-in
> prompt does not need first-boot setup to have *run*, it needs the console to
> be free.

You can list more than one trigger, including more than one of the same type.
`["timer:*-*-* 02:00:00", "timer:*-*-* 14:00:00"]` runs at 2 am and 2 pm. The
model is built to grow — future trigger types (path, device, event) will slot
into the same list without a schema change.

## Where to start

To see how a timer-started run appears as a job and an operation, read [Jobs
and operations](~peios/services-and-jobs/jobs-and-operations).

To understand the states a timer-started service moves through, read [The
service lifecycle](~peios/services-and-jobs/the-service-lifecycle).

For the exact registry keys that store last-run timestamps, see the [Registry
key reference](~peios/services-and-jobs/registry-key-reference).
