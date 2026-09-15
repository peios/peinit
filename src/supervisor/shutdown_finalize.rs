use crate::boundary::{BoundaryError, ShutdownFinalizer};
use crate::shutdown::{
    CleanupActionResult, MountCleanupResult, ShutdownError, ShutdownFinalizationReport,
    ShutdownFinalizationState, ShutdownRuntime,
};

use super::dispatch::SupervisorShutdownFinalizationDispatch;
use super::state::{Supervisor, SupervisorError};

const FINAL_ACTION_RETRY_INTERVAL_NS: u64 = 1_000_000_000;

impl Supervisor {
    pub fn finalize_shutdown<F>(
        &mut self,
        finalizer: &mut F,
        now_ns: u64,
    ) -> Result<SupervisorShutdownFinalizationDispatch, SupervisorError>
    where
        F: ShutdownFinalizer + ?Sized,
    {
        let state = self.shutdown_runtime()?.finalization.clone();

        match state {
            ShutdownFinalizationState::Ready => self.run_shutdown_finalization(finalizer, now_ns),
            ShutdownFinalizationState::Failed {
                next_retry_at_ns, ..
            } if now_ns >= next_retry_at_ns => self.retry_shutdown_final_action(finalizer, now_ns),
            ShutdownFinalizationState::Failed {
                next_retry_at_ns, ..
            } => Err(SupervisorError::Shutdown(
                ShutdownError::FinalActionRetryNotDue {
                    next_retry_at_ns,
                    now_ns,
                },
            )),
            ShutdownFinalizationState::WaitingForServices => {
                Err(SupervisorError::Shutdown(ShutdownError::ShutdownNotReady))
            }
            ShutdownFinalizationState::Completed => {
                Err(SupervisorError::Shutdown(ShutdownError::ShutdownNotReady))
            }
        }
    }

    fn run_shutdown_finalization<F>(
        &mut self,
        finalizer: &mut F,
        now_ns: u64,
    ) -> Result<SupervisorShutdownFinalizationDispatch, SupervisorError>
    where
        F: ShutdownFinalizer + ?Sized,
    {
        let kind = self.shutdown_runtime()?.kind;
        let random_seed = cleanup_result(finalizer.save_random_seed());
        let (snapshot_mounts, mount_points) = snapshot_mounts(finalizer);
        let mount_results = cleanup_mounts(finalizer, mount_points);
        let root_remount = cleanup_result(finalizer.remount_readonly("/"));
        let sync_result = cleanup_result(finalizer.sync_filesystems());
        let reboot_result = cleanup_result(finalizer.reboot(kind));
        let attempted_at_ns = time_after_attempt(finalizer, now_ns);
        let report = ShutdownFinalizationReport {
            random_seed,
            snapshot_mounts,
            mount_results,
            root_remount,
            sync_result,
            reboot_result: reboot_result.clone(),
        };
        let finalization = finalization_after_reboot_result(reboot_result, attempted_at_ns);
        self.shutdown_runtime_mut()?.finalization = finalization.clone();

        Ok(SupervisorShutdownFinalizationDispatch {
            report,
            finalization,
        })
    }

    fn retry_shutdown_final_action<F>(
        &mut self,
        finalizer: &mut F,
        now_ns: u64,
    ) -> Result<SupervisorShutdownFinalizationDispatch, SupervisorError>
    where
        F: ShutdownFinalizer + ?Sized,
    {
        let kind = self.shutdown_runtime()?.kind;
        let sync_result = cleanup_result(finalizer.sync_filesystems());
        let reboot_result = cleanup_result(finalizer.reboot(kind));
        let attempted_at_ns = time_after_attempt(finalizer, now_ns);
        let report = ShutdownFinalizationReport {
            random_seed: CleanupActionResult::Ok,
            snapshot_mounts: CleanupActionResult::Ok,
            mount_results: Vec::new(),
            root_remount: CleanupActionResult::Ok,
            sync_result,
            reboot_result: reboot_result.clone(),
        };
        let finalization = finalization_after_reboot_result(reboot_result, attempted_at_ns);
        self.shutdown_runtime_mut()?.finalization = finalization.clone();

        Ok(SupervisorShutdownFinalizationDispatch {
            report,
            finalization,
        })
    }

    pub(super) fn finalize_without_mount_cleanup<F>(
        &mut self,
        finalizer: &mut F,
        now_ns: u64,
    ) -> Result<SupervisorShutdownFinalizationDispatch, SupervisorError>
    where
        F: ShutdownFinalizer + ?Sized,
    {
        self.retry_shutdown_final_action(finalizer, now_ns)
    }

    fn shutdown_runtime(&self) -> Result<&ShutdownRuntime, SupervisorError> {
        self.shutdown.as_ref().ok_or(SupervisorError::Shutdown(
            ShutdownError::NoShutdownInProgress,
        ))
    }

    fn shutdown_runtime_mut(&mut self) -> Result<&mut ShutdownRuntime, SupervisorError> {
        self.shutdown.as_mut().ok_or(SupervisorError::Shutdown(
            ShutdownError::NoShutdownInProgress,
        ))
    }
}

fn snapshot_mounts(
    finalizer: &mut (impl ShutdownFinalizer + ?Sized),
) -> (CleanupActionResult, Vec<String>) {
    match finalizer.snapshot_mounts() {
        Ok(mut mount_points) => {
            mount_points.retain(|mount_point| mount_point != "/");
            mount_points.sort_by(|left, right| {
                mount_depth(right)
                    .cmp(&mount_depth(left))
                    .then_with(|| left.cmp(right))
            });
            (CleanupActionResult::Ok, mount_points)
        }
        Err(error) => (cleanup_error(error), Vec::new()),
    }
}

fn cleanup_mounts(
    finalizer: &mut (impl ShutdownFinalizer + ?Sized),
    mount_points: Vec<String>,
) -> Vec<MountCleanupResult> {
    mount_points
        .into_iter()
        .map(|mount_point| {
            let unmount = cleanup_result(finalizer.unmount(&mount_point));
            let remount_readonly = match unmount {
                CleanupActionResult::Ok => None,
                CleanupActionResult::Failed(_) => {
                    Some(cleanup_result(finalizer.remount_readonly(&mount_point)))
                }
            };
            MountCleanupResult {
                mount_point,
                unmount,
                remount_readonly,
            }
        })
        .collect()
}

/// When the final action returned, for timing its retry.
///
/// The turn's `now_ns` predates the seed write, the unmounts and the remount,
/// so a retry timed from it came before a second had passed since the
/// attempt (PEI-1088). The finalizer's clock, read after `reboot(2)`
/// returned, is used where it has one; the turn's time is the floor either
/// way, so a clock that went backwards cannot bring the retry forward.
fn time_after_attempt(finalizer: &mut (impl ShutdownFinalizer + ?Sized), now_ns: u64) -> u64 {
    finalizer
        .monotonic_now_ns()
        .map_or(now_ns, |after_ns| after_ns.max(now_ns))
}

fn finalization_after_reboot_result(
    reboot_result: CleanupActionResult,
    attempted_at_ns: u64,
) -> ShutdownFinalizationState {
    match reboot_result {
        CleanupActionResult::Ok => ShutdownFinalizationState::Completed,
        CleanupActionResult::Failed(message) => ShutdownFinalizationState::Failed {
            message,
            next_retry_at_ns: attempted_at_ns.saturating_add(FINAL_ACTION_RETRY_INTERVAL_NS),
        },
    }
}

fn cleanup_result(result: Result<(), BoundaryError>) -> CleanupActionResult {
    result
        .map(|_| CleanupActionResult::Ok)
        .unwrap_or_else(cleanup_error)
}

fn cleanup_error(error: BoundaryError) -> CleanupActionResult {
    CleanupActionResult::Failed(format!("{error:?}"))
}

fn mount_depth(mount_point: &str) -> usize {
    mount_point
        .split('/')
        .filter(|segment| !segment.is_empty())
        .count()
}
