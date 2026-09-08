//! A launch queue entry whose job has gone must not be fatal.
//!
//! The queues hold job ids awaiting launch; the job store holds the records.
//! Keeping them in step used to be an invariant maintained by hand at every
//! path that finishes a `Created` job, and missing one was punished out of all
//! proportion: the drain looked the id up, got `UnknownJob`, and that error
//! left peinit's runtime loop — which ends PID 1 (PEI-605).
//!
//! The queue is now a hint and the store is the truth, so the invariant is
//! unnecessary rather than merely unenforced.

use crate::job::JobState;
use crate::supervisor::{Supervisor, SupervisorServiceLaunchDispatch, SupervisorSettings};

use super::{
    APP_LAUNCH_NS, AUTHD_LAUNCH_NS, BOOT_NS, ScriptedClock, StaticRegistry, TestProcessLauncher,
    TestTokenProvider, alive_service, process, settings,
};

/// Boot two independent services, so two ids sit in the launch queue.
fn booted_with_two_queued_jobs() -> (Supervisor, Vec<crate::ids::JobId>) {
    let mut supervisor = Supervisor::new(SupervisorSettings::new(settings()));
    let mut registry = StaticRegistry::services(vec![alive_service("app"), alive_service("other")]);
    let mut clock = ScriptedClock::new([BOOT_NS, AUTHD_LAUNCH_NS, APP_LAUNCH_NS]);
    supervisor
        .run_phase2_boot(&mut registry, &mut clock)
        .expect("boot supervisor");
    let queued = supervisor.pending_launch_jobs();
    assert_eq!(queued.len(), 2, "both services queue a main job");
    (supervisor, queued)
}

/// The regression: this used to return `UnknownJob`, and that error is fatal.
#[test]
fn a_queued_job_whose_record_has_gone_is_dropped_not_fatal() {
    let (mut supervisor, queued) = booted_with_two_queued_jobs();
    let stale = queued[0];
    assert_eq!(
        supervisor.jobs().get(stale).expect("job").state,
        JobState::Created
    );

    // Finish the job without touching the queue — the mistake any one of the
    // dozen finish paths could make.
    supervisor
        .jobs_mut()
        .fail_job_before_start(stale, APP_LAUNCH_NS, "cancelled")
        .expect("finish job");
    assert!(supervisor.jobs().get(stale).is_none());
    assert_eq!(supervisor.pending_launch_jobs()[0], stale);

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
    let mut clock = ScriptedClock::new([APP_LAUNCH_NS + 1]);
    let launched = supervisor
        .launch_next_pending_service_job(&mut tokens, &mut launcher, &mut clock)
        .expect("draining a stale entry must not be an error");

    // It skipped the rubbish and launched the live job behind it, rather than
    // giving up at the first bad id.
    let Some(SupervisorServiceLaunchDispatch::Launched(dispatch)) = launched else {
        panic!("expected the live job behind the stale entry to launch");
    };
    assert_eq!(dispatch.launch.job_id, queued[1]);
    assert!(supervisor.pending_launch_jobs().is_empty());
    assert_eq!(
        supervisor.take_stale_launch_entries(),
        1,
        "the drop is counted, so a bookkeeping fault stays visible",
    );
}

/// Every queued id being stale is still not an error — there is simply
/// nothing to launch.
#[test]
fn an_entirely_stale_queue_drains_to_nothing() {
    let (mut supervisor, queued) = booted_with_two_queued_jobs();
    for job_id in &queued {
        supervisor
            .jobs_mut()
            .fail_job_before_start(*job_id, APP_LAUNCH_NS, "cancelled")
            .expect("finish job");
    }

    let mut tokens = TestTokenProvider::default();
    let mut launcher = TestProcessLauncher::new(vec![process(4242, 9)]);
    let mut clock = ScriptedClock::new([APP_LAUNCH_NS + 1]);
    let launched = supervisor
        .launch_next_pending_service_job(&mut tokens, &mut launcher, &mut clock)
        .expect("draining must not be an error");

    assert!(launched.is_none());
    assert!(supervisor.pending_launch_jobs().is_empty());
    assert_eq!(supervisor.take_stale_launch_entries(), 2);
}
