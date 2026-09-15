use crate::control::connection::{
    ControlConnectionAdmission, ControlConnectionAdmissionDecision, ControlConnectionRecord,
    ControlConnectionTable, ControlConnectionTableError, ControlOperationWait, ControlPendingWait,
    control_connection_admission_decision,
};
use crate::control::system::ControlPeer;
use crate::ids::OperationIdAllocator;
use crate::security::TokenSummary;

#[test]
fn connection_admission_rejects_when_active_count_reaches_limit() {
    assert_eq!(
        control_connection_admission_decision(31, 32),
        ControlConnectionAdmissionDecision::Accept,
    );
    assert_eq!(
        control_connection_admission_decision(32, 32),
        ControlConnectionAdmissionDecision::RejectAtSocket,
    );
    assert_eq!(
        control_connection_admission_decision(0, 0),
        ControlConnectionAdmissionDecision::RejectAtSocket,
    );
}

#[test]
fn connection_table_admits_until_limit_then_returns_rejected_record() {
    let mut table = ControlConnectionTable::new(2);

    assert_eq!(
        table.admit(10, "first").expect("first"),
        ControlConnectionAdmission::Accepted {
            fd: 10,
            active_connections: 1,
        },
    );
    assert_eq!(
        table.admit(11, "second").expect("second"),
        ControlConnectionAdmission::Accepted {
            fd: 11,
            active_connections: 2,
        },
    );
    assert_eq!(
        table.admit(12, "third").expect("third rejected"),
        ControlConnectionAdmission::RejectedAtSocket {
            fd: 12,
            record: "third",
            active_connections: 2,
            max_connections: 2,
        },
    );
    assert_eq!(table.len(), 2);
    assert_eq!(table.get(10), Some(&"first"));
    assert_eq!(table.get(12), None);
}

#[test]
fn connection_table_removes_records_and_rejects_duplicate_fds() {
    let mut table = ControlConnectionTable::new(2);

    table.admit(10, "first").expect("first");
    assert_eq!(
        table.admit(10, "duplicate").expect_err("duplicate fd"),
        ControlConnectionTableError::AlreadyTracked { fd: 10 },
    );
    assert_eq!(table.remove(10), Some("first"));
    assert!(table.is_empty());
    assert_eq!(
        table.admit(10, "replacement").expect("replacement"),
        ControlConnectionAdmission::Accepted {
            fd: 10,
            active_connections: 1,
        },
    );
    *table.get_mut(10).expect("replacement record") = "mutated";
    assert_eq!(table.get(10), Some(&"mutated"));
}

#[test]
fn idle_deadlines_ignore_pending_waits_and_pending_writes() {
    let mut table = ControlConnectionTable::new(4);
    table
        .admit(10, record_with_activity(1_000_000_000))
        .expect("idle connection");
    table
        .admit(11, record_with_activity(2_000_000_000))
        .expect("waiting connection");
    table
        .admit(12, record_with_activity(3_000_000_000))
        .expect("writing connection");

    let operation_id = OperationIdAllocator::new()
        .allocate_batch(1, 1_000_000_000)
        .expect("operation id")
        .remove(0);
    table
        .get_mut(11)
        .expect("waiting connection")
        .state_mut()
        .set_pending_wait(ControlPendingWait::Operation(ControlOperationWait {
            operation_id,
            service: "app".to_string(),
        }));
    table
        .get_mut(12)
        .expect("writing connection")
        .state_mut()
        .enqueue_response(b"{}\n", false);

    assert!(table.has_pending_waits());
    assert_eq!(table.next_idle_deadline_ns(5), Some(6_000_000_000));
    assert_eq!(table.idle_fds(5_999_999_999, 5), Vec::<i32>::new());
    assert_eq!(table.idle_fds(6_000_000_000, 5), vec![10]);
}

#[test]
fn buffered_frame_keeps_a_connection_out_of_the_idle_count() {
    let mut table = ControlConnectionTable::new(4);
    table
        .admit(10, record_with_activity(1_000_000_000))
        .expect("idle connection");
    table
        .admit(11, record_with_activity(1_000_000_000))
        .expect("connection with a buffered request");
    table
        .admit(12, record_with_activity(1_000_000_000))
        .expect("connection with a partial request");
    table
        .get_mut(11)
        .expect("buffered connection")
        .state_mut()
        .read_buffer_mut()
        .append(b"{\"command\":\"list\"}\n");
    table
        .get_mut(12)
        .expect("partial connection")
        .state_mut()
        .read_buffer_mut()
        .append(b"{\"command\":\"li");

    // A complete request that nothing has answered is work in flight; a
    // partial one is not, so a stalled writer still times out.
    assert_eq!(table.fds_with_runnable_frames(), vec![11]);
    assert_eq!(table.idle_fds(6_000_000_000, 5), vec![10, 12]);
    assert_eq!(table.next_idle_deadline_ns(5), Some(6_000_000_000));

    table
        .get_mut(11)
        .expect("buffered connection")
        .state_mut()
        .read_buffer_mut()
        .clear();
    assert!(table.fds_with_runnable_frames().is_empty());
    assert_eq!(table.idle_fds(6_000_000_000, 5), vec![10, 11, 12]);
}

#[test]
fn zero_second_idle_timeout_expires_immediately_after_activity() {
    let mut table = ControlConnectionTable::new(1);
    table
        .admit(10, record_with_activity(1_000_000_000))
        .expect("connection");

    assert_eq!(table.next_idle_deadline_ns(0), Some(1_000_000_000));
    assert_eq!(table.idle_fds(1_000_000_000, 0), vec![10]);
}

fn record_with_activity(observed_at_ns: u64) -> ControlConnectionRecord<()> {
    ControlConnectionRecord::new_with_activity((), peer(), Some(observed_at_ns))
}

fn peer() -> ControlPeer {
    ControlPeer::borrowed_token_fd(1, TokenSummary::requested_identity("test"))
}
