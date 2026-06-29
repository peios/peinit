use crate::job::{
    JobEventDetail, JobRecord, JobState, JobStore, JobStoreError, ProcessHandle, ServiceMainJobSpec,
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
