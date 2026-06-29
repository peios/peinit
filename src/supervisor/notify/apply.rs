use crate::boundary::ProcessController;
use crate::execution::notify::{
    AuthenticatedNotifySender, NotifyApplyContext, NotifyApplyError, NotifyApplyRequest,
    apply_notify_message, authenticate_notify_sender,
};
use crate::notify::{NotifyDatagram, parse_notify_message};

use super::fd_store::apply_fd_store_notify_fields;
use super::shutdown_timeout::apply_shutdown_timeout_extensions;
use super::timeout_extension::apply_transition_timeout_extensions;
use crate::supervisor::dispatch::SupervisorNotifyDispatch;
use crate::supervisor::health::apply_health_scheduling_after_transitions;
use crate::supervisor::relationships::apply_relationship_reactions_after_transitions;
use crate::supervisor::state::{Supervisor, SupervisorError};
use crate::supervisor::watchdog::{
    apply_watchdog_notify_fields, apply_watchdog_scheduling_after_transitions,
};
use crate::supervisor::work::SupervisorWork;

impl Supervisor {
    pub fn authenticate_notify_datagram_sender<P>(
        &self,
        sender_pid: u32,
        controller: &mut P,
    ) -> Result<AuthenticatedNotifySender, NotifyApplyError>
    where
        P: ProcessController + ?Sized,
    {
        authenticate_notify_sender(&self.services, &self.jobs, controller, sender_pid)
    }

    pub fn apply_notify_datagram<P>(
        &mut self,
        datagram: NotifyDatagram,
        observed_at_ns: u64,
        controller: &mut P,
    ) -> Result<SupervisorNotifyDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        let NotifyDatagram {
            payload,
            credentials,
            fds,
        } = datagram;
        let message = parse_notify_message(&payload).map_err(SupervisorError::NotifyParse)?;
        let mut work = SupervisorWork::from_supervisor(self);
        let notify = apply_notify_message(
            NotifyApplyContext {
                services: &mut work.services,
                operations: &mut work.operations,
                graph: &mut work.graph,
                jobs: &mut work.jobs,
                job_ids: &mut work.job_ids,
                start_store: &mut work.start,
                control_store: &mut work.control,
                controller,
            },
            NotifyApplyRequest {
                sender_pid: credentials.pid,
                observed_at_ns,
            },
            &message,
        )
        .map_err(SupervisorError::Notify)?;
        let fd_store_rejections = apply_fd_store_notify_fields(&mut work, &message, &notify, fds)
            .map_err(SupervisorError::Notify)?;
        apply_shutdown_timeout_extensions(&mut work, &notify, observed_at_ns)
            .map_err(SupervisorError::Shutdown)?;
        apply_transition_timeout_extensions(&mut work, &notify, observed_at_ns)
            .map_err(SupervisorError::TimeoutExtension)?;
        if let Some(post_start_hook) = &notify.post_start_hook {
            work.queue_created_post_hook_job(post_start_hook);
        }
        if notify.post_start_hook.is_none() {
            apply_health_scheduling_after_transitions(
                &mut work,
                &notify.service_transitions,
                observed_at_ns,
            );
            apply_watchdog_scheduling_after_transitions(
                &mut work,
                &notify.service_transitions,
                observed_at_ns,
            );
        }
        let watchdog_notifications =
            apply_watchdog_notify_fields(&mut work, &notify, observed_at_ns)
                .map_err(SupervisorError::Watchdog)?;
        let mut start_dispatches = apply_relationship_reactions_after_transitions(
            &mut work,
            &notify.service_transitions,
            observed_at_ns,
            self.settings.phase2.max_parallel_starts,
        )?;
        start_dispatches.extend(work.release_after_graph_events(
            &notify.graph_events,
            self.settings.phase2.max_parallel_starts,
            observed_at_ns,
        )?);

        work.commit(self);

        Ok(SupervisorNotifyDispatch {
            notify,
            fd_store_rejections,
            watchdog_notifications,
            start_dispatches,
        })
    }
}
