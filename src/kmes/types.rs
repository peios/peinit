//! peinit's event types and their tiers, as `peinit.evman` declares them.
//!
//! The one place a type's name and tier are written in code. A unit test
//! holds this table and the fragment to the same set of types and tiers.

use crate::boundary::EventTier;

pub const JOB_CREATED: &str = "peinit.job.created";
pub const JOB_STARTED: &str = "peinit.job.started";
pub const JOB_ENDED: &str = "peinit.job.ended";
pub const JOB_STATUS_REPORTED: &str = "peinit.job.status.reported";
pub const JOB_OUTPUT_DROPPED: &str = "peinit.job.output.dropped";
pub const OPERATION_REQUESTED: &str = "peinit.operation.requested";
pub const OPERATION_STARTED: &str = "peinit.operation.started";
pub const OPERATION_ENDED: &str = "peinit.operation.ended";
pub const OPERATION_MERGED: &str = "peinit.operation.merged";
pub const GRAPH_OPERATION_ENDED: &str = "peinit.graph.operation.ended";
pub const GRAPH_VALIDATION_FAILED: &str = "peinit.graph.validation.failed";
pub const GRAPH_VALIDATION_WARNED: &str = "peinit.graph.validation.warned";
pub const BOOT_DOWNGRADED: &str = "peinit.boot.downgraded";
pub const RECOVERY_ENTERED: &str = "peinit.recovery.entered";
pub const CRITICAL_SERVICE_FAILED: &str = "peinit.critical-service.failed";
pub const SERVICE_ABANDONED: &str = "peinit.service.abandoned";
pub const CGROUP_LEAKED: &str = "peinit.cgroup.leaked";
pub const ON_FAILURE_SUPPRESSED: &str = "peinit.on-failure.suppressed";
pub const SERVICE_RELOAD_TIMED_OUT: &str = "peinit.service.reload.timed-out";
pub const INTERNAL_ERROR_CONTAINED: &str = "peinit.internal-error.contained";
pub const EVENT_DROPPED: &str = "peinit.event.dropped";
pub const CONFIG_RELOAD_DEFERRED: &str = "peinit.config.reload.deferred";
pub const CONFIG_RELOAD_APPLIED: &str = "peinit.config.reload.applied";
pub const NOTIFY_STATUS_REPORTED: &str = "peinit.notify.status.reported";
pub const NOTIFY_ERRNO_REPORTED: &str = "peinit.notify.errno.reported";
pub const NOTIFY_EXIT_STATUS_REPORTED: &str = "peinit.notify.exit-status.reported";
pub const NOTIFY_STOPPING_REPORTED: &str = "peinit.notify.stopping.reported";
pub const NOTIFY_PROGRESS_REPORTED: &str = "peinit.notify.progress.reported";
pub const NOTIFY_REJECTED: &str = "peinit.notify.rejected";
pub const FD_STORE_REJECTED: &str = "peinit.fd-store.rejected";

/// Every type peinit emits, with its tier.
pub const EVENT_TYPES: &[(&str, EventTier)] = &[
    (JOB_CREATED, EventTier::Verbose),
    (JOB_STARTED, EventTier::Standard),
    (JOB_ENDED, EventTier::Standard),
    (JOB_STATUS_REPORTED, EventTier::Verbose),
    (JOB_OUTPUT_DROPPED, EventTier::Standard),
    (OPERATION_REQUESTED, EventTier::Standard),
    (OPERATION_STARTED, EventTier::Verbose),
    (OPERATION_ENDED, EventTier::Standard),
    (OPERATION_MERGED, EventTier::Verbose),
    (GRAPH_OPERATION_ENDED, EventTier::Verbose),
    (GRAPH_VALIDATION_FAILED, EventTier::Standard),
    (GRAPH_VALIDATION_WARNED, EventTier::Standard),
    (BOOT_DOWNGRADED, EventTier::Essential),
    (RECOVERY_ENTERED, EventTier::Essential),
    (CRITICAL_SERVICE_FAILED, EventTier::Essential),
    (SERVICE_ABANDONED, EventTier::Standard),
    (CGROUP_LEAKED, EventTier::Standard),
    (ON_FAILURE_SUPPRESSED, EventTier::Standard),
    (SERVICE_RELOAD_TIMED_OUT, EventTier::Standard),
    (INTERNAL_ERROR_CONTAINED, EventTier::Standard),
    (EVENT_DROPPED, EventTier::Essential),
    (CONFIG_RELOAD_DEFERRED, EventTier::Verbose),
    (CONFIG_RELOAD_APPLIED, EventTier::Standard),
    (NOTIFY_STATUS_REPORTED, EventTier::Verbose),
    (NOTIFY_ERRNO_REPORTED, EventTier::Standard),
    (NOTIFY_EXIT_STATUS_REPORTED, EventTier::Standard),
    (NOTIFY_STOPPING_REPORTED, EventTier::Standard),
    (NOTIFY_PROGRESS_REPORTED, EventTier::Verbose),
    (NOTIFY_REJECTED, EventTier::Standard),
    (FD_STORE_REJECTED, EventTier::Standard),
];

/// The tier of one of peinit's event types. A type missing from the table
/// is a peinit bug; it is treated as `standard`, the tier that is on unless
/// switched off, and panics in a debug build.
pub fn tier_of(event_type: &str) -> EventTier {
    match EVENT_TYPES.iter().find(|(name, _)| *name == event_type) {
        Some((_, tier)) => *tier,
        None => {
            debug_assert!(false, "{event_type} is not one of peinit's event types");
            EventTier::Standard
        }
    }
}
