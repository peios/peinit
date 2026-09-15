use crate::job::{
    JobEventDetail, JobExit, JobRecord, JobState, JobStore, JobStoreError, ProcessHandle,
    ServiceMainJobSpec,
};

use super::{job_ids, operation_ids, service, service_main_job, token_summary};

#[test]
fn store_tracks_active_job_then_drops_it_on_terminal_event() {
    let mut store = JobStore::new();
    let job = service_main_job();
    let id = job.id;

    let created = store.create_job(job).expect("create");
    assert_eq!(created.job_id, id);
    assert_eq!(
        created.detail,
        JobEventDetail::Created {
            image_path: "/sbin/app".to_string(),
            identity: "LocalService".to_string(),
            operation_id: Some(operation_ids(1)[0]),
        }
    );
    assert_eq!(store.active_for_service("app"), vec![id]);
    assert_eq!(store.current_service_main_job("app"), Some(id));

    let started = store
        .start_job(
            id,
            ProcessHandle {
                pid: 1234,
                pidfd: 9,
            },
            1_100,
        )
        .expect("start");
    assert_eq!(
        started.detail,
        JobEventDetail::Started {
            started_at_ns: 1_100,
            pid: 1234,
            cgroup_id: "/sys/fs/cgroup/peinit/app/main".to_string(),
        }
    );

    let ended = store.complete_job(id, 1_500, 0).expect("complete");
    assert_eq!(ended.state, JobState::Completed);
    assert_eq!(
        ended.detail,
        JobEventDetail::Ended {
            ended_at_ns: 1_500,
            duration_ns: 500,
            exit_code: Some(0),
            exit_signal: None,
            failure_cause: None,
        }
    );
    assert!(store.get(id).is_none());
    assert!(store.active_for_service("app").is_empty());
    assert_eq!(store.active_job_by_pid(1234), None);
}

#[test]
fn store_resolves_running_job_by_pid() {
    let mut store = JobStore::new();
    let job = service_main_job();
    let id = job.id;
    store.create_job(job).expect("create");
    assert_eq!(store.active_job_by_pid(1234), None);

    store
        .start_job(
            id,
            ProcessHandle {
                pid: 1234,
                pidfd: 9,
            },
            1_100,
        )
        .expect("start");

    assert_eq!(store.active_job_by_pid(1234), Some(id));
    assert_eq!(store.active_job_by_pid(4321), None);
}

#[test]
fn store_rejects_duplicate_id_and_second_active_service_main() {
    let mut store = JobStore::new();
    let ids = job_ids(2);
    let operations = operation_ids(2);
    let first = JobRecord::new_service_main(
        ids[0],
        ServiceMainJobSpec {
            service: &service(),
            resolved_identity: "LocalService".to_string(),
            token_summary: token_summary("LocalService"),
            activation_generation: 0,
            cgroup_generation: 0,
            operation_id: operations[0],
            created_at_ns: 1_000,
        },
    );
    let duplicate = first.clone();
    let second = JobRecord::new_service_main(
        ids[1],
        ServiceMainJobSpec {
            service: &service(),
            resolved_identity: "LocalService".to_string(),
            token_summary: token_summary("LocalService"),
            activation_generation: 0,
            cgroup_generation: 0,
            operation_id: operations[1],
            created_at_ns: 1_000,
        },
    );

    store.create_job(first).expect("create first");

    assert_eq!(
        store.create_job(duplicate).expect_err("duplicate"),
        JobStoreError::DuplicateJobId { id: ids[0] },
    );
    assert_eq!(
        store.create_job(second).expect_err("service-main active"),
        JobStoreError::ServiceMainAlreadyActive {
            service: "app".to_string(),
            active_job_id: ids[0],
        },
    );
}

#[test]
fn store_drops_failed_before_start_job_after_terminal_event() {
    let mut store = JobStore::new();
    let job = service_main_job();
    let id = job.id;

    store.create_job(job).expect("create");
    let event = store
        .fail_job_before_start(id, 1_020, "ParentSetupFailure: pipe2")
        .expect("fail before start");

    assert_eq!(event.state, JobState::Failed);
    assert_eq!(
        event.detail,
        JobEventDetail::Ended {
            ended_at_ns: 1_020,
            duration_ns: 20,
            exit_code: None,
            exit_signal: None,
            failure_cause: Some("ParentSetupFailure: pipe2".to_string()),
        }
    );
    assert!(store.get(id).is_none());
    assert!(store.active_for_service("app").is_empty());
}

fn started_store(pidfd: i32) -> (JobStore, crate::ids::JobId) {
    let mut store = JobStore::new();
    let job = service_main_job();
    let id = job.id;
    store.create_job(job).expect("create");
    store
        .start_job(id, ProcessHandle { pid: 1234, pidfd }, 1_100)
        .expect("start");
    (store, id)
}

// PEI-816: the record owns the pidfd from `start`, so every path out of the
// store releases it exactly once -- before this, the descriptor went away with
// the dropped record and PID 1 held one more `anon_inode:[pidfd]` per
// activation for the rest of the boot.
#[test]
fn store_releases_pidfd_when_a_running_job_completes() {
    let (mut store, id) = started_store(9);
    assert!(store.take_released_pidfds().is_empty());

    store.complete_job(id, 1_500, 0).expect("complete");

    assert_eq!(store.take_released_pidfds(), vec![9]);
    assert!(store.take_released_pidfds().is_empty(), "handed out once");
}

#[test]
fn store_releases_pidfd_when_a_running_job_fails() {
    let (mut store, id) = started_store(11);

    store
        .fail_running_job(id, 1_500, Some(JobExit::Signal(9)), "watchdog timed out")
        .expect("fail running");

    assert_eq!(store.take_released_pidfds(), vec![11]);
}

#[test]
fn store_releases_pidfd_when_a_running_job_is_abandoned() {
    let (mut store, id) = started_store(13);

    store
        .abandon_job(id, 1_500, "ProcessUnkillable")
        .expect("abandon");

    assert_eq!(store.take_released_pidfds(), vec![13]);
}

#[test]
fn store_releases_nothing_for_a_job_that_never_started() {
    let mut store = JobStore::new();
    let job = service_main_job();
    let id = job.id;
    store.create_job(job).expect("create");

    store
        .fail_job_before_start(id, 1_020, "ParentSetupFailure: pipe2")
        .expect("fail before start");

    assert!(store.take_released_pidfds().is_empty());
}

#[test]
fn store_carries_released_pidfds_through_a_clone() {
    // The supervisor finishes jobs on a clone and commits the clone; a release
    // recorded there must survive the commit and not be duplicated by it.
    let (store, id) = started_store(17);
    let mut work = store.clone();
    work.complete_job(id, 1_500, 0).expect("complete");
    let mut committed = work;

    assert_eq!(committed.take_released_pidfds(), vec![17]);
    let mut original = store;
    assert!(original.take_released_pidfds().is_empty());
}
