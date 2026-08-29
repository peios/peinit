use serde_json::json;

use crate::ids::JobIdAllocator;
use crate::job::{JobEvent, JobRecord, JobState, SubmittedJobSpec};
use crate::security::TokenSummary;

use super::security::JobSecurityDescriptor;
use super::{
    JobIdentity, JobProgress, JobProgressUnit, JobReadiness, ProgressParseError, SubmittedJobCause,
    SubmittedJobDeadlineKind, SubmittedJobDefinitionError, SubmittedJobEntry,
    SubmittedJobListFilter, SubmittedJobStore, job_view, parse_progress, parse_progress_unit,
    parse_submitted_job_definition,
};

const CREATED_NS: u64 = 1_000_000_000;

fn object(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    value.as_object().cloned().expect("object")
}

#[test]
fn definition_defaults_are_the_specified_ones() {
    let definition = parse_submitted_job_definition(&object(json!({"image_path": "/bin/true"})), 0)
        .expect("definition");
    assert_eq!(definition.image_path, "/bin/true");
    assert!(definition.arguments.is_empty());
    assert_eq!(definition.working_directory, "/");
    assert_eq!(definition.timeout_secs, 0);
    assert_eq!(definition.stop_timeout_secs, 10);
    assert_eq!(definition.readiness, JobReadiness::None);
    assert_eq!(definition.readiness_timeout_secs, 30);
    assert!(!definition.output);
    assert_eq!(definition.security_descriptor_sddl, None);
}

#[test]
fn definition_rejects_relative_and_nul_paths() {
    let err = parse_submitted_job_definition(&object(json!({"image_path": "bin/true"})), 0)
        .expect_err("relative");
    assert!(matches!(
        err,
        SubmittedJobDefinitionError::InvalidField {
            field: "image_path",
            ..
        }
    ));
    let err = parse_submitted_job_definition(
        &object(json!({"image_path": "/bin/true", "working_directory": "/tmp\u{0}x"})),
        0,
    )
    .expect_err("nul");
    assert!(matches!(
        err,
        SubmittedJobDefinitionError::InvalidField {
            field: "working_directory",
            ..
        }
    ));
}

#[test]
fn definition_rejects_environment_names_with_equals() {
    let err = parse_submitted_job_definition(
        &object(json!({"image_path": "/bin/true", "environment": {"A=B": "c"}})),
        0,
    )
    .expect_err("equals");
    assert!(matches!(
        err,
        SubmittedJobDefinitionError::InvalidField {
            field: "environment",
            ..
        }
    ));
}

#[test]
fn definition_rejects_zero_stop_timeout_and_unknown_readiness() {
    let err = parse_submitted_job_definition(
        &object(json!({"image_path": "/bin/true", "stop_timeout": 0})),
        0,
    )
    .expect_err("stop_timeout");
    assert!(matches!(
        err,
        SubmittedJobDefinitionError::InvalidField {
            field: "stop_timeout",
            ..
        }
    ));
    let err = parse_submitted_job_definition(
        &object(json!({"image_path": "/bin/true", "readiness": "alive"})),
        0,
    )
    .expect_err("readiness");
    assert!(matches!(
        err,
        SubmittedJobDefinitionError::InvalidField {
            field: "readiness",
            ..
        }
    ));
}

#[test]
fn definition_descriptor_count_includes_the_output_sink() {
    let request = json!({"image_path": "/bin/true", "descriptors": ["session"], "output": true});
    assert!(parse_submitted_job_definition(&object(request.clone()), 2).is_ok());
    let err = parse_submitted_job_definition(&object(request), 1).expect_err("count");
    assert_eq!(
        err,
        SubmittedJobDefinitionError::DescriptorCountMismatch {
            expected: 2,
            attached: 1
        }
    );
}

#[test]
fn definition_rejects_descriptor_names_with_colons() {
    let err = parse_submitted_job_definition(
        &object(json!({"image_path": "/bin/true", "descriptors": ["a:b"]})),
        1,
    )
    .expect_err("colon");
    assert!(matches!(
        err,
        SubmittedJobDefinitionError::InvalidField {
            field: "descriptors",
            ..
        }
    ));
}

#[test]
fn definition_rejects_exit_codes_outside_a_byte() {
    let err = parse_submitted_job_definition(
        &object(json!({"image_path": "/bin/true", "success_exit_codes": [256]})),
        0,
    )
    .expect_err("range");
    assert!(matches!(
        err,
        SubmittedJobDefinitionError::InvalidField {
            field: "success_exit_codes",
            ..
        }
    ));
}

#[test]
fn progress_grammar_has_three_forms() {
    assert_eq!(
        parse_progress("7").expect("count"),
        JobProgress {
            current: 7,
            total: None,
            bounded: false
        }
    );
    assert_eq!(
        parse_progress("7/").expect("pending total"),
        JobProgress {
            current: 7,
            total: None,
            bounded: true
        }
    );
    assert_eq!(
        parse_progress("7/10").expect("bounded"),
        JobProgress {
            current: 7,
            total: Some(10),
            bounded: true
        }
    );
}

#[test]
fn progress_grammar_rejects_what_the_spec_rejects() {
    assert_eq!(parse_progress("7/0"), Err(ProgressParseError::ZeroTotal));
    assert_eq!(
        parse_progress("11/10"),
        Err(ProgressParseError::CurrentExceedsTotal)
    );
    assert_eq!(parse_progress(""), Err(ProgressParseError::Malformed));
    assert_eq!(parse_progress("+7"), Err(ProgressParseError::Malformed));
    assert_eq!(parse_progress("7/10/1"), Err(ProgressParseError::Malformed));
    assert_eq!(parse_progress("0.5"), Err(ProgressParseError::Malformed));
    assert_eq!(parse_progress_unit("bytes"), Some(JobProgressUnit::Bytes));
    assert_eq!(parse_progress_unit("furlongs"), None);
}

fn entry(
    store: &mut SubmittedJobStore,
    timeout_secs: u64,
    readiness: JobReadiness,
) -> SubmittedJobEntry {
    let mut ids = JobIdAllocator::new();
    let job_id = ids.allocate_batch(1, CREATED_NS).expect("id")[0];
    let mut definition = parse_submitted_job_definition(
        &object(json!({"image_path": "/bin/true", "timeout": timeout_secs})),
        0,
    )
    .expect("definition");
    definition.readiness = readiness;
    let entry = SubmittedJobEntry {
        job_id,
        submitter_sid: "S-1-5-80-1".to_string(),
        identity: JobIdentity {
            user_sid: "S-1-5-21-1".to_string(),
            logon_session: 1042,
        },
        definition,
        security_descriptor: JobSecurityDescriptor { bytes: vec![1] },
        created_at_ns: CREATED_NS,
        prepared_token_fd: None,
        attached_descriptors: Vec::new(),
        output_sink_fd: None,
        ready: (readiness == JobReadiness::Notify).then_some(false),
        status_text: None,
        progress: None,
        progress_unit: None,
        last_status_event_ns: None,
        stopping_acknowledged: false,
        cause: None,
        stop: None,
        outcome: None,
        cgroup_id: format!("/sys/fs/cgroup/peinit/jobs/{job_id}"),
        cgroup_cleanup_due_at_ns: None,
        output_drop_reported: false,
    };
    store.insert(entry.clone()).expect("insert");
    entry
}

fn record(entry: &SubmittedJobEntry) -> JobRecord {
    JobRecord::new_submitted(
        entry.job_id,
        SubmittedJobSpec {
            identity_user_sid: entry.identity.user_sid.clone(),
            token_summary: TokenSummary::requested_identity("S-1-5-21-1"),
            image_path: entry.definition.image_path.clone(),
            arguments: Vec::new(),
            environment: Vec::new(),
            working_directory: "/".to_string(),
            created_at_ns: CREATED_NS,
        },
    )
}

#[test]
fn timeout_and_readiness_deadlines_count_from_the_start() {
    let mut store = SubmittedJobStore::new();
    let entry = entry(&mut store, 5, JobReadiness::Notify);
    assert_eq!(
        store.next_deadline(|_| None),
        None,
        "no deadline before exec"
    );
    let started = CREATED_NS + 1;
    let next = store.next_deadline(|_| Some(started)).expect("deadline");
    assert_eq!(next.kind, SubmittedJobDeadlineKind::Timeout);
    assert_eq!(next.due_at_ns, started + 5_000_000_000);
    let due = store.due_deadlines(started + 30_000_000_000, |_| Some(started));
    assert_eq!(due.len(), 2);
    assert_eq!(due[0].kind, SubmittedJobDeadlineKind::Timeout);
    assert_eq!(due[1].kind, SubmittedJobDeadlineKind::ReadinessTimeout);
    store.get_mut(entry.job_id).expect("entry").ready = Some(true);
    let due = store.due_deadlines(started + 30_000_000_000, |_| Some(started));
    assert_eq!(due.len(), 1, "a ready job has no readiness deadline");
}

#[test]
fn a_stop_replaces_the_other_deadlines_and_is_not_restarted() {
    let mut store = SubmittedJobStore::new();
    let entry = entry(&mut store, 5, JobReadiness::None);
    let started = CREATED_NS + 1;
    assert!(
        store
            .begin_stop(entry.job_id, SubmittedJobCause::ExplicitStop, started + 10)
            .expect("stop")
    );
    let next = store.next_deadline(|_| Some(started)).expect("deadline");
    assert_eq!(next.kind, SubmittedJobDeadlineKind::StopKill);
    assert_eq!(next.due_at_ns, started + 10 + 10_000_000_000);
    assert!(
        !store
            .begin_stop(entry.job_id, SubmittedJobCause::Timeout, started + 20)
            .expect("second stop")
    );
    assert_eq!(
        store.get(entry.job_id).expect("entry").cause,
        Some(SubmittedJobCause::ExplicitStop),
        "the first cause stands"
    );
    store
        .record_kill(entry.job_id, started + 100, 5)
        .expect("kill");
    let next = store.next_deadline(|_| Some(started)).expect("deadline");
    assert_eq!(next.kind, SubmittedJobDeadlineKind::PostKill);
    assert_eq!(next.due_at_ns, started + 100 + 5_000_000_000);
}

#[test]
fn the_outcome_is_retained_then_purged() {
    let mut store = SubmittedJobStore::new();
    let entry = entry(&mut store, 0, JobReadiness::None);
    let mut record = record(&entry);
    record
        .start(
            crate::job::ProcessHandle { pid: 42, pidfd: 9 },
            CREATED_NS + 1,
        )
        .expect("start");
    record.complete(CREATED_NS + 2, 0).expect("complete");
    let event = JobEvent::ended(&record).expect("event");
    let retained = store
        .record_terminal(&event, 60_000_000_000)
        .expect("terminal");
    assert_eq!(
        retained.outcome.as_ref().expect("outcome").state,
        JobState::Completed
    );
    assert!(!retained.is_live());
    assert_eq!(store.live_count_for_submitter("S-1-5-80-1"), 0);
    assert_eq!(
        store.next_retention_deadline_ns(),
        Some(CREATED_NS + 2 + 60_000_000_000)
    );
    assert!(
        store
            .purge_retained_until(CREATED_NS + 2 + 59_000_000_000)
            .is_empty()
    );
    assert_eq!(
        store.purge_retained_until(CREATED_NS + 2 + 60_000_000_000),
        vec![entry.job_id]
    );
    assert!(store.get(entry.job_id).is_none());
}

#[test]
fn a_setup_failure_derives_its_cause_from_the_record() {
    let mut store = SubmittedJobStore::new();
    let entry = entry(&mut store, 0, JobReadiness::None);
    let mut record = record(&entry);
    record
        .fail_before_start(CREATED_NS + 1, "PreExecFailure: exec failed")
        .expect("fail");
    let event = JobEvent::ended(&record).expect("event");
    let retained = store.record_terminal(&event, 1).expect("terminal");
    assert_eq!(retained.cause, Some(SubmittedJobCause::PreExecFailure));
    let view = job_view(retained, None).expect("view");
    assert_eq!(view.state.state, JobState::Failed);
    assert_eq!(view.state.started_at_ns, None);
    assert_eq!(view.state.exit_code, None);
}

#[test]
fn the_view_of_a_live_job_comes_from_its_record() {
    let mut store = SubmittedJobStore::new();
    let entry = entry(&mut store, 0, JobReadiness::None);
    let mut record = record(&entry);
    record
        .start(
            crate::job::ProcessHandle { pid: 42, pidfd: 9 },
            CREATED_NS + 1,
        )
        .expect("start");
    let view = job_view(&entry, Some(&record)).expect("view");
    assert_eq!(view.state.state, JobState::Running);
    assert_eq!(view.state.pid, Some(42));
    assert_eq!(view.identity_sid, "S-1-5-21-1");
    assert_eq!(view.logon_session, 1042);
    assert!(job_view(&entry, None).is_none());
}

#[test]
fn list_filters_all_hold_together() {
    let mut store = SubmittedJobStore::new();
    let entry = entry(&mut store, 0, JobReadiness::None);
    let all = SubmittedJobListFilter::default();
    assert_eq!(
        store.filtered_ids(&all, |_| JobState::Running),
        vec![entry.job_id]
    );
    let wrong_session = SubmittedJobListFilter {
        logon_session: Some(7),
        ..SubmittedJobListFilter::default()
    };
    assert!(
        store
            .filtered_ids(&wrong_session, |_| JobState::Running)
            .is_empty()
    );
    let right_both = SubmittedJobListFilter {
        submitter_sid: Some("S-1-5-80-1".to_string()),
        state: Some(JobState::Running),
        ..SubmittedJobListFilter::default()
    };
    assert_eq!(
        store.filtered_ids(&right_both, |_| JobState::Running),
        vec![entry.job_id]
    );
}
