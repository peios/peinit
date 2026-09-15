use super::*;
use crate::boot::phase2::Phase2BootSettings;
use crate::boundary::{LinuxTimerFd, TimerLastRunWriteOutcome};
use crate::runtime::linux::calendar_timer::entry::LinuxCalendarTimerEntry;
use crate::runtime::linux::calendar_timer::test_support::{
    FixedClock, TimerHistory, TimerWriteRecorder, ns_utc,
};
use crate::service::{ServiceDefinition, ServiceTrigger};
use crate::supervisor::SupervisorSettings;
use crate::timer::calendar::CalendarSchedule;
use crate::timer::state::TimerLastRunStorage;

const EVERY_MINUTE: &str = "*-*-* *:*:00 UTC";
const MINUTE_NS: u64 = 60 * NANOS_PER_SEC;

/// PEI-831. A per-minute schedule with a ten-minute jitter: every firing is
/// delayed by up to the whole window, and each one must still be followed by
/// the *next* occurrence, not the first occurrence after the delayed firing.
/// Five occurrences produce five firings, each at or after its occurrence and
/// within its jitter window.
#[test]
fn a_jittered_firing_rearms_for_the_occurrence_after_the_one_that_fired() {
    let jitter_secs = 600;
    let first_ns = ns_utc(2024, 5, 3, 12, 0, 0);
    let mut supervisor = timer_supervisor(false, jitter_secs, first_ns);
    let mut table = LinuxCalendarTimerTable::new();
    let fd = insert_entry(&mut table, false, jitter_secs, first_ns);
    let mut writer = TimerWriteRecorder::default();

    let mut scheduled_ns = first_ns;
    for occurrence in 0..5 {
        let armed_ns = table.entries[&fd].armed_deadline_ns;
        assert!(
            (scheduled_ns..=scheduled_ns + jitter_secs * NANOS_PER_SEC).contains(&armed_ns),
            "occurrence {occurrence} is armed outside its jitter window",
        );
        // The firing lands at the far end of the window: later than the next
        // few occurrences, which jitter delays but must not drop.
        let fired_at_ns = scheduled_ns + jitter_secs * NANOS_PER_SEC;
        let mut clock = FixedClock {
            monotonic_ns: 10_000 + occurrence,
            realtime_ns: fired_at_ns,
        };

        let turn = table
            .fire_and_rearm_with_realtime(
                &mut supervisor,
                fd,
                LinuxTimerFdRead::Expired { expirations: 1 },
                &mut clock,
                &mut writer,
                fired_at_ns,
            )
            .expect("fire and rearm");

        let RuntimeCalendarTimerTurn::Read {
            next_scheduled_ns: Some(next_scheduled_ns),
            ..
        } = turn
        else {
            panic!("expected a timer read turn");
        };
        assert_eq!(
            next_scheduled_ns,
            scheduled_ns + MINUTE_NS,
            "occurrence {} was skipped after a jittered firing",
            occurrence + 1,
        );
        assert_eq!(table.entries[&fd].next_scheduled_ns, next_scheduled_ns);
        scheduled_ns = next_scheduled_ns;
    }
}

/// The other half of §9.4: an occurrence whose whole jitter window elapsed
/// before the firing was handled is missed, not replayed. Without jitter the
/// anchor is simply now, exactly as before.
#[test]
fn an_occurrence_outside_the_jitter_window_is_not_replayed() {
    let scheduled_ns = ns_utc(2024, 5, 3, 12, 0, 0);

    // No jitter: the search starts from now.
    assert_eq!(
        rearm_anchor_ns(scheduled_ns, 0, scheduled_ns + 7 * MINUTE_NS),
        scheduled_ns + 7 * MINUTE_NS,
    );
    // Jitter covers the delay: the search starts from the occurrence.
    assert_eq!(
        rearm_anchor_ns(scheduled_ns, 600, scheduled_ns + 7 * MINUTE_NS),
        scheduled_ns,
    );
    // Jitter does not cover the delay: only occurrences still inside a
    // jitter window are eligible.
    assert_eq!(
        rearm_anchor_ns(scheduled_ns, 120, scheduled_ns + 7 * MINUTE_NS),
        scheduled_ns + 5 * MINUTE_NS,
    );
    // A firing handled before its occurrence is never anchored earlier.
    assert_eq!(
        rearm_anchor_ns(scheduled_ns, 120, scheduled_ns - 1),
        scheduled_ns,
    );
}

fn timer_supervisor(persistent: bool, jitter_secs: u64, now_ns: u64) -> Supervisor {
    let mut service = ServiceDefinition::simple_system_boot("app", "/sbin/app");
    service.triggers = vec![ServiceTrigger::Timer {
        schedule: EVERY_MINUTE.to_string(),
    }];
    service.timer_persistent = persistent;
    service.timer_jitter_secs = jitter_secs;
    let mut supervisor = Supervisor::new(SupervisorSettings::new(Phase2BootSettings {
        max_parallel_starts: 10,
        ..Phase2BootSettings::default()
    }));
    let mut registry = TimerHistory::new(vec![service], now_ns);
    let mut clock = FixedClock {
        monotonic_ns: 1,
        realtime_ns: now_ns,
    };
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    supervisor
}

/// PEI-1083. §9.3: `TimerPersistent=0` ignores history entirely; §14.4: the
/// last-run write forks one child per firing of a *persistent* timer. A
/// non-persistent firing used to queue the write anyway, forking PID 1 for
/// a value nothing would ever read.
#[test]
fn a_non_persistent_firing_does_not_write_last_run() {
    let scheduled_ns = ns_utc(2024, 5, 3, 12, 0, 0);
    let mut supervisor = timer_supervisor(false, 0, scheduled_ns);
    let mut table = LinuxCalendarTimerTable::new();
    let fd = insert_entry(&mut table, false, 0, scheduled_ns);
    let mut writer = TimerWriteRecorder::default();
    let mut clock = FixedClock {
        monotonic_ns: 10_000,
        realtime_ns: scheduled_ns,
    };

    let turn = table
        .fire_and_rearm_with_realtime(
            &mut supervisor,
            fd,
            LinuxTimerFdRead::Expired { expirations: 1 },
            &mut clock,
            &mut writer,
            scheduled_ns,
        )
        .expect("fire and rearm");

    assert!(
        writer.writes.is_empty(),
        "a non-persistent firing forked a write"
    );
    let RuntimeCalendarTimerTurn::Read {
        supervisor: Some(_),
        last_run_write: None,
        next_scheduled_ns: Some(next_scheduled_ns),
        ..
    } = turn
    else {
        panic!("expected a dispatched firing with no last-run write, got {turn:?}");
    };
    assert_eq!(next_scheduled_ns, scheduled_ns + MINUTE_NS);
}

#[test]
fn a_persistent_firing_writes_last_run_once() {
    let scheduled_ns = ns_utc(2024, 5, 3, 12, 0, 0);
    let mut supervisor = timer_supervisor(true, 0, scheduled_ns);
    let mut table = LinuxCalendarTimerTable::new();
    let fd = insert_entry(&mut table, true, 0, scheduled_ns);
    let mut writer = TimerWriteRecorder::default();
    let mut clock = FixedClock {
        monotonic_ns: 10_000,
        realtime_ns: scheduled_ns,
    };

    let turn = table
        .fire_and_rearm_with_realtime(
            &mut supervisor,
            fd,
            LinuxTimerFdRead::Expired { expirations: 1 },
            &mut clock,
            &mut writer,
            scheduled_ns,
        )
        .expect("fire and rearm");

    assert_eq!(writer.writes.len(), 1);
    assert_eq!(writer.writes[0].timestamp_realtime_ns, scheduled_ns);
    assert!(matches!(
        turn,
        RuntimeCalendarTimerTurn::Read {
            last_run_write: Some(Ok(TimerLastRunWriteOutcome::Queued { pid: 4242 })),
            ..
        }
    ));
}

fn insert_entry(
    table: &mut LinuxCalendarTimerTable,
    persistent: bool,
    jitter_secs: u64,
    next_scheduled_ns: u64,
) -> i32 {
    let timer = LinuxTimerFd::create_realtime().expect("timerfd");
    let armed_deadline_ns = jittered_deadline_ns(next_scheduled_ns, jitter_secs).expect("jitter");
    timer
        .arm_realtime_absolute_ns(armed_deadline_ns)
        .expect("arm timerfd");
    let fd = timer.as_raw_fd();
    table.entries.insert(
        fd,
        LinuxCalendarTimerEntry {
            timer,
            service: "app".to_string(),
            schedule: EVERY_MINUTE.to_string(),
            storage: TimerLastRunStorage::SingleTimer,
            persistent,
            calendar: CalendarSchedule::parse(EVERY_MINUTE).expect("schedule"),
            next_scheduled_ns,
            armed_deadline_ns,
            jitter_secs,
        },
    );
    fd
}
