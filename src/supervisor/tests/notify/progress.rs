use crate::execution::notify::NotifyAppliedField;
use crate::service::runtime::ServiceProgressReport;
use crate::submitted::{JobProgress, JobProgressUnit, PROGRESS_EVENT_INTERVAL_NS};

use super::{NOTIFY_NS, apply_notify, datagram, notify_app_supervisor};

const THREE_OF_TEN: JobProgress = JobProgress {
    current: 3,
    total: Some(10),
    bounded: true,
};

/// PSPU §4.19: a service's `PROGRESS` and `PROGRESS_UNIT` are retained and
/// exposed as `progress` in its status, and a change of progress becomes a
/// `notify.progress` event no more than once a second.
#[test]
fn a_services_progress_is_retained_exposed_and_bounded_as_an_event() {
    let mut supervisor = notify_app_supervisor();
    assert_eq!(
        supervisor.service_status("app").expect("app").progress,
        None
    );

    let dispatch = apply_notify(
        &mut supervisor,
        datagram(8000, b"PROGRESS=3/10\nPROGRESS_UNIT=items"),
        NOTIFY_NS,
    )
    .expect("apply progress");
    assert_eq!(
        dispatch.notify.applied_fields,
        vec![
            NotifyAppliedField::Progress {
                value: "3/10".to_string()
            },
            NotifyAppliedField::ProgressUnit {
                value: "items".to_string()
            },
        ],
    );
    assert_eq!(
        dispatch.notify.progress_event,
        Some(ServiceProgressReport {
            progress: Some(THREE_OF_TEN),
            unit: Some(JobProgressUnit::Items),
        }),
        "one event for the datagram, carrying both as retained after it",
    );
    let view = supervisor.service_status("app").expect("app");
    assert_eq!(view.progress, Some(THREE_OF_TEN));
    assert_eq!(view.progress_unit, Some(JobProgressUnit::Items));

    // Within the second: retained, but no event.
    let dispatch = apply_notify(
        &mut supervisor,
        datagram(8000, b"PROGRESS=4/10"),
        NOTIFY_NS + 1,
    )
    .expect("apply second progress");
    assert_eq!(dispatch.notify.progress_event, None);
    assert_eq!(
        supervisor.service_status("app").expect("app").progress,
        Some(JobProgress {
            current: 4,
            total: Some(10),
            bounded: true,
        }),
    );
    assert_eq!(
        supervisor.service_status("app").expect("app").progress_unit,
        Some(JobProgressUnit::Items),
        "a unit is replaced only by a datagram that carries one",
    );

    // A second on: the latest becomes an event.
    let dispatch = apply_notify(
        &mut supervisor,
        datagram(8000, b"PROGRESS=5/"),
        NOTIFY_NS + PROGRESS_EVENT_INTERVAL_NS,
    )
    .expect("apply third progress");
    assert_eq!(
        dispatch.notify.progress_event,
        Some(ServiceProgressReport {
            progress: Some(JobProgress {
                current: 5,
                total: None,
                bounded: true,
            }),
            unit: Some(JobProgressUnit::Items),
        }),
    );
}

/// An unexpected `PROGRESS` or `PROGRESS_UNIT` is ignored, never repaired,
/// and the rest of the datagram applies (§4.17, §4.19). A `STATUS` alone
/// leaves `progress` as it was and is due no progress event.
#[test]
fn an_unexpected_progress_value_is_ignored_and_the_rest_applies() {
    let mut supervisor = notify_app_supervisor();
    apply_notify(&mut supervisor, datagram(8000, b"PROGRESS=3/10"), NOTIFY_NS)
        .expect("apply progress");

    let dispatch = apply_notify(
        &mut supervisor,
        datagram(
            8000,
            b"PROGRESS=5/3\nPROGRESS=1/0\nPROGRESS=x\nPROGRESS_UNIT=furlongs\nSTATUS=Scanning",
        ),
        NOTIFY_NS + PROGRESS_EVENT_INTERVAL_NS,
    )
    .expect("apply unexpected values");

    assert_eq!(
        dispatch.notify.applied_fields,
        vec![NotifyAppliedField::Status {
            text: "Scanning".to_string()
        }],
    );
    assert_eq!(dispatch.notify.progress_event, None);
    let view = supervisor.service_status("app").expect("app");
    assert_eq!(view.progress, Some(THREE_OF_TEN));
    assert_eq!(view.progress_unit, None);
    assert_eq!(view.status_text.as_deref(), Some("Scanning"));
}
