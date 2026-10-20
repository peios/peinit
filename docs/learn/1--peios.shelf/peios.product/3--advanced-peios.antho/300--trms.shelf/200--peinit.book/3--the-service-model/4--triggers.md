---
title: Triggers
description: What makes a service start by itself — the trigger kinds, how arity is enforced, and how the set is extended.
---

A trigger says when a service starts by itself. Triggers are independent
of service type: a Simple service can have a timer, and a Oneshot can
start at boot. [*trig.triggers-are-independent-of-service-type]

`Triggers` is a multi_string, each entry either `type` or
`type:argument`. [*trig.a-trigger-entry-is-a-type-or-a-type-and-argument]

| Trigger | Form | Meaning |
|---|---|---|
| Boot | `boot` | Start during the Phase 2 boot sequence. |
| Deferred boot | `boot:settled` | Start once the boot set has settled, or the deadline expires. §2.5 |
| Terminal handover | `tty:released` | Start when this service's `TTYPath` is let go of. §11.6 |
| Timer | `timer:<schedule>` | Start on a schedule. §9.1 |

A service with no triggers is demand-only: it starts only when something
asks for it, whether an administrator, a dependency, or an `OnFailure`
handler.

Multiple triggers of the same type are allowed.
[*trig.multiple-triggers-of-one-type-are-allowed] A service with
`["timer:*-*-* 02:00:00", "timer:*-*-* 14:00:00"]` runs at 2am and 2pm,
and each trigger is independent — its own timerfd, its own next-firing
computation, its own history.

## Arity is enforced

`boot` takes no argument. `boot:<something>` accepts only the
sub-triggers in the table above, and any other value is malformed rather
than an unknown trigger type to be ignored.
[*trig.boot-accepts-only-the-listed-sub-triggers] `tty` carries the same
rule: the bare word names no event, and `tty:` accepts only `released`.
[*trig.a-bare-tty-is-malformed-and-tty-accepts-only-released] `timer`
with no schedule is malformed, as is `timer:` with an empty one.
[*trig.a-timer-with-no-schedule-is-malformed]

That strictness is the point. Unknown *values* on a service key are
ignored for forward compatibility, and if unknown trigger types were
ignored too, `boot:setled` would be silently accepted and the service
would never start. Instead it fails to decode, loudly.

## Triggers that need a field

`tty:released` is about the terminal this service names, so a definition
carrying it without a `TTYPath` is refused rather than ignored
[*trig.tty-released-without-a-ttypath-is-refused] — it describes a
service that would silently never start, and starting is the entire
point of the trigger. `TTYPrecedence` is refused on the same grounds.
[*trig.ttyprecedence-without-a-ttypath-is-refused] Emptying `TTYPath`
(§3.3) takes both down with it.
[*trig.emptying-ttypath-takes-both-down-with-it]

## Disabled

`Disabled=1` suppresses automatic activation and nothing else. No
trigger fires: not `boot`, not `boot:settled`, not `tty:released`, not a
timer, and not any trigger type added later.
[*trig.disabled-suppresses-every-trigger] A disabled service's timers
are neither armed nor serviced.
[*trig.a-disabled-services-timers-are-not-armed]

The definition is still loaded into the in-memory model, and an explicit
start still works. [*trig.a-disabled-service-can-still-be-started-explicitly]
To stop a service being started at all, deny
`SERVICE_START` in its `ServiceSecurity` descriptor — that is an access
control question, not a trigger question, and answering it with the
`Disabled` flag would mean a flag that anyone who can write the key can
clear.

Enabling and disabling are registry writes performed by administrative
tools, not peinit commands. peinit picks the change up through a
registry change notification, or on the next reload-config.

## Extensibility

The `type:argument` shape is designed to grow. Path, device and event
triggers slot into the same array with no schema change,
[*trig.a-new-trigger-type-needs-no-schema-change] because a trigger is a
string in a list rather than a field of its own.
