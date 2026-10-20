---
title: Calendar Expressions
description: The OnCalendar grammar peinit parses — component syntax, named shortcuts, timezones, daylight saving,  and precision.
---

A timer schedule is a calendar expression in systemd's `OnCalendar`
format. The grammar below is what peinit parses.

```text
DayOfWeek Year-Month-Day Hour:Minute:Second Timezone
```

Every field is optional, and which fields are present is worked out
positionally from their shape.
[*cal.fields-are-optional-and-identified-positionally]

| Field | Form | Default if omitted |
|---|---|---|
| DayOfWeek | weekday names | any day |
| Date | `Year-Month-Day` | `*-*-*` |
| Time | `Hour:Minute:Second` | `00:00:00` |
| Second | the `:Second` of Time | `:00` |
| Timezone | an IANA name | system-local |

A time-only expression such as `02:00:00` implies the date `*-*-*`.
[*cal.a-time-only-expression-implies-every-date]
`Hour:Minute` alone defaults the seconds to `00`.
[*cal.hour-minute-defaults-the-seconds-to-zero]

Years range 0–9999, months 1–12, days 1–31.
[*cal.numeric-components-have-fixed-ranges]

## Component syntax

Every numeric component, and the weekday, accepts:
<!-- lint-disable-next-line line-length -->
[*cal.numeric-components-and-the-weekday-take-wildcards-lists-ranges-and-steps]

- **Wildcard** — `*` matches anything.
- **List** — comma-separated values: `1,15`.
- **Range** — two values around `..`, inclusive: `Mon..Fri`, `8..17`.
  A reversed range is a parse error rather than a wrap-around.
  [*cal.a-reversed-range-is-a-parse-error]
- **Repetition** — a value or range suffixed with `/` and a step.
  `value/step` matches the value and every multiple of the step above
  it, so `0/15` in the minute field is 0, 15, 30 and 45.
  `start..end/step` walks from start to end inclusive.
  [*cal.a-step-matches-every-multiple-above-its-start]

Weekdays are English names, case-insensitive, abbreviated or full, and
accept lists and ranges.
[*cal.weekday-names-are-english-case-insensitive-and-abbreviable]
`Mon` and `Monday` are the same day, as are `Tue`, `Tues` and `Tuesday`.

## Last day of the month

A `~` in place of the `-` between Month and Day counts the day from the
**end** of the month: `~01` is the last day, `~02` the second-to-last,
and so on. [*cal.a-tilde-counts-the-day-from-the-end-of-the-month]
`*-*~01` is the last day of every month; `*-02~03` is the
third-to-last day of February.

Repetition combines with it, and steps in the day-of-month direction —
which means it walks the offset *downwards*, toward the end of the
month. [*cal.a-step-after-a-tilde-walks-the-offset-downwards]
`Mon *-05~07/1` covers the last seven days of May, and combined
with `Mon` resolves to exactly one day: the last Monday in May.

Wildcards, ranges and lists are also accepted after `~`, so `~*` matches
every day and `~03..07` the third- to seventh-to-last.
[*cal.wildcards-ranges-and-lists-are-accepted-after-a-tilde]

<!-- lint-disable-next-line line-length -->
## Named shortcuts [*cal.the-named-shortcuts-expand-to-the-equivalent-expression]

| Shortcut | Equivalent |
|---|---|
| `minutely` | `*-*-* *:*:00` |
| `hourly` | `*-*-* *:00:00` |
| `daily` | `*-*-* 00:00:00` |
| `weekly` | `Mon *-*-* 00:00:00` |
| `monthly` | `*-*-01 00:00:00` |
| `quarterly` | `*-01,04,07,10-01 00:00:00` |
| `semiannually` | `*-01,07-01 00:00:00` |
| `yearly`, `annually` | `*-01-01 00:00:00` |

Shortcut names are case-insensitive and accept a trailing timezone, so
`daily UTC` is valid.
[*cal.shortcut-names-are-case-insensitive-and-take-a-trailing-timezone]

## Timezones

Timezone specifiers are IANA database names — `Europe/London`,
`US/Eastern`, `UTC`. An expression with no timezone is interpreted in
system-local time.
[*cal.a-timezone-is-an-iana-name-and-its-absence-means-system-local]

A name is validated when the expression is parsed and resolved again at
evaluation. An unrecognized zone is a hard parse error, not a silent
fallback to UTC. [*cal.an-unrecognised-timezone-is-a-parse-error]

## Daylight saving

- **Spring forward**, where the clock skips an hour: a scheduled time
  falling inside the skipped interval does not fire. peinit moves on to
  the next scheduled second, then the next date.
  [*cal.a-time-in-the-spring-forward-gap-does-not-fire]
  A schedule of `*-03-31 01:30 Europe/London` skips 2024 entirely.
- **Fall back**, where an hour repeats: a scheduled time inside the
  repeated interval fires exactly once, on the **first** occurrence.
  [*cal.a-time-in-the-repeated-hour-fires-once-on-the-first-occurrence]
  On re-arming, the same civil time resolves to the same instant, which
  is not later than the firing that occurred, so the timer advances
  to the next day rather than firing again.

## Precision

Second-level. Unlike systemd, peinit does not accept fractional seconds
— service scheduling has no use for finer granularity, and dropping it
keeps the parser simpler. A fraction anywhere in the time component is a
parse error. [*cal.a-fraction-in-the-time-component-is-a-parse-error]

## Examples

| Expression | Meaning |
|---|---|
| `*-*-* 02:00:00` | Every day at 2am, system-local. |
| `Mon *-*-* 00:00:00` | Every Monday at midnight. |
| `*-*-1,15 12:00:00` | The 1st and 15th, at noon. |
| `*-*~01 00:00:00` | The last day of each month, at midnight. |
| `Mon..Fri *-*-* 09:00:00` | Weekdays at 9am. |
| `*-*-* *:00/15:00` | Every fifteen minutes. |
| `*-*-* 02:00:00 Europe/London` | Every day at 2am London time. |

> [!NOTE]
> An expression that parses but can never match — `*-02-30`, or a fixed
> year already past — is not rejected at parse time.
> [*cal.an-unsatisfiable-expression-is-not-a-parse-error]
> Computing its next occurrence walks forward a day at a time for ten
> years and then gives up, reporting no future occurrence.
> [*cal.an-unsatisfiable-expression-is-walked-for-ten-years] The horizon
> is there because walking the full year range would turn an
> unsatisfiable schedule into a boot that hangs.
