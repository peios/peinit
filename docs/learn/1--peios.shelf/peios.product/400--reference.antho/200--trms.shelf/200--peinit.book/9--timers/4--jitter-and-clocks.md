---
title: Jitter and Clocks
description: The random delay applied to each firing, which clock a timer is evaluated against, and what a clock change does.
---

## Jitter

`TimerJitter`, zero by default, adds a random delay to each firing.
peinit draws a uniformly random whole number of seconds from zero to
`TimerJitter` inclusive, from the kernel's random source, and adds it to
the computed occurrence.
<!-- lint-disable-next-line line-length -->
[*jitter.a-random-delay-of-zero-to-timerjitter-seconds-is-added-to-each-firing]

The delay is recomputed on every firing, so a daily timer with
`TimerJitter=900` fires at a different moment between 00:00 and 00:15
each day. [*jitter.the-delay-is-recomputed-on-every-firing]
With `TimerJitter=0` no randomness is consulted at all.
[*jitter.zero-jitter-consults-no-randomness]

Jitter is applied **after** the calendar expression is evaluated and is
only ever added, so a timer never fires early — only late.
[*jitter.a-timer-never-fires-early]

The boot catch-up firing is not jittered. It fires immediately, and
jitter applies from the next armed occurrence onward.
[*jitter.the-boot-catch-up-firing-is-not-jittered]

## Which clock

The split is the point of this section, and it is not cosmetic.

**Calendar timers are wall-clock schedules**, so they are armed as
absolute `CLOCK_REALTIME` timers: `timerfd_settime` with
`TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET`. `CANCEL_ON_SET` makes the
descriptor's read return `ECANCELED` whenever the realtime clock is
discontinuously changed — an NTP step, a manual set. peinit recomputes
the next occurrence against the new wall clock and re-arms, which is
what keeps `*-*-* 02:00:00` anchored to 02:00 across clock corrections.
<!-- lint-disable-next-line spelling line-length -->
[*jitter.calendar-timers-are-absolute-realtime-timers-cancelled-on-a-clock-set]

**Interval timers are genuine relative durations** and use
`CLOCK_MONOTONIC`: the watchdog, health check intervals and timeouts,
restart backoff, and the Start, Stop and Reload phase timeouts. "Wait
thirty seconds" means thirty elapsed seconds regardless of what happens
to the wall clock. [*jitter.interval-timers-are-monotonic]
They are not armed with `CANCEL_ON_SET`, correctly —
a monotonic timer has no reason to be canceled by a realtime set.

These are not separate descriptors. Every interval deadline is
aggregated onto one monotonic timerfd armed to the earliest of them.

**Last-run timestamps are recorded on `CLOCK_REALTIME` **, since they
record when a timer actually fired in wall-clock terms.
[*jitter.last-run-timestamps-are-recorded-on-the-realtime-clock]
The same firing
passes a monotonic timestamp into the operation machinery, because
operation timing is elapsed time.

## Clock events

- **A realtime step at runtime.** The armed timer is canceled; peinit
  recomputes against the new wall clock and re-arms. A backward step
  pushes the next firing later;
  [*jitter.a-backward-step-pushes-the-next-firing-later]
  a forward step that crosses an occurrence fires it once.
  [*jitter.a-forward-step-across-an-occurrence-fires-it-once]
  If the step lands inside a jitter window,
  the firing happens at the un-jittered scheduled time — later than the
  schedule, never earlier.
  [*jitter.a-step-inside-a-jitter-window-fires-at-the-unjittered-time]
- **Suspend and resume.** An absolute deadline that elapsed while
  suspended fires once on resume. The expiration count is ignored, so a
  long suspend produces one firing, not one per occurrence.
  [*jitter.an-elapsed-absolute-deadline-fires-once-on-resume]
- **A missed occurrence within one uptime.** Fire once, then compute the
  next future occurrence. peinit never replays every occurrence that
  elapsed during a gap — the same rule as the cross-reboot catch-up.
  [*jitter.a-missed-occurrence-within-one-uptime-fires-once]
- **A backward jump across a boot.** If the last-run timestamp is in the
  future relative to the current wall clock at boot, peinit treats the
  history as unknown and fires the catch-up immediately. This check is
  boot-time only; there is no runtime equivalent.
  [*jitter.a-last-run-timestamp-in-the-future-is-treated-as-unknown]
- **A wrong clock at boot.** A system that boots with a badly wrong
  clock and has NTP correct it later can fire a persistent catch-up
  spuriously or not at all. The runtime half is covered by
  `CANCEL_ON_SET` — once NTP corrects the clock, armed calendar timers
  are canceled and recomputed — but the boot-time catch-up decision has
  already been made by then.
  [*jitter.a-boot-time-catch-up-decision-is-never-revisited]
  Short of NTP-aware rescheduling, this remains an edge.

> [!NOTE]
> The calendar parser deserves heavy testing. Time parsing is a rich
> source of edge cases: month boundaries, leap years,
> last-day-of-month arithmetic, DST transitions and timezone database
> updates are all fertile ground for subtle errors, and every one of
> them is a bug that only appears on a particular day of a particular
> year.
