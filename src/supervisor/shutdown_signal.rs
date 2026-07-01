use crate::boundary::{Clock, LinuxSignalFdRead, ProcessController, ShutdownFinalizer};
use crate::shutdown::{ShutdownKind, ShutdownSignal};

use super::dispatch::{SupervisorShutdownSignalAction, SupervisorShutdownSignalDispatch};
use super::state::{Supervisor, SupervisorError};

impl Supervisor {
    pub fn handle_pid1_signal_fd_read<C, P, F>(
        &mut self,
        read: LinuxSignalFdRead,
        clock: &mut C,
        controller: &mut P,
        finalizer: &mut F,
    ) -> Result<SupervisorPid1SignalFdTurn, SupervisorError>
    where
        C: Clock + ?Sized,
        P: ProcessController + ?Sized,
        F: ShutdownFinalizer,
    {
        match read {
            LinuxSignalFdRead::Shutdown(signal) => {
                let observed_at_ns = clock.monotonic_ns().map_err(SupervisorError::Clock)?;
                self.handle_shutdown_signal(signal, controller, finalizer, observed_at_ns)
                    .map(Box::new)
                    .map(SupervisorPid1SignalFdTurn::Shutdown)
            }
            LinuxSignalFdRead::Other { signal } => Ok(SupervisorPid1SignalFdTurn::Other { signal }),
            LinuxSignalFdRead::WouldBlock => Ok(SupervisorPid1SignalFdTurn::WouldBlock),
        }
    }

    pub fn handle_shutdown_signal<P, F>(
        &mut self,
        signal: ShutdownSignal,
        controller: &mut P,
        finalizer: &mut F,
        observed_at_ns: u64,
    ) -> Result<SupervisorShutdownSignalDispatch, SupervisorError>
    where
        P: ProcessController + ?Sized,
        F: ShutdownFinalizer,
    {
        let action = match signal {
            ShutdownSignal::Sigterm | ShutdownSignal::Sigpwr => {
                self.handle_poweroff_signal(controller, observed_at_ns)?
            }
            ShutdownSignal::Sigint => self.handle_sigint(controller, finalizer, observed_at_ns)?,
        };
        Ok(SupervisorShutdownSignalDispatch { signal, action })
    }

    fn handle_poweroff_signal<P>(
        &mut self,
        controller: &mut P,
        observed_at_ns: u64,
    ) -> Result<SupervisorShutdownSignalAction, SupervisorError>
    where
        P: ProcessController + ?Sized,
    {
        if let Some(shutdown) = &self.shutdown {
            return Ok(SupervisorShutdownSignalAction::AlreadyInProgress {
                kind: shutdown.kind,
            });
        }
        self.begin_shutdown(ShutdownKind::Poweroff, controller, observed_at_ns)
            .map(SupervisorShutdownSignalAction::Graceful)
    }

    fn handle_sigint<P, F>(
        &mut self,
        controller: &mut P,
        finalizer: &mut F,
        observed_at_ns: u64,
    ) -> Result<SupervisorShutdownSignalAction, SupervisorError>
    where
        P: ProcessController + ?Sized,
        F: ShutdownFinalizer,
    {
        if self.shutdown_signals.record_sigint(observed_at_ns) {
            return self
                .force_reboot_shutdown(controller, finalizer, observed_at_ns)
                .map(SupervisorShutdownSignalAction::Forced);
        }
        if let Some(shutdown) = &self.shutdown {
            return Ok(SupervisorShutdownSignalAction::AlreadyInProgress {
                kind: shutdown.kind,
            });
        }
        self.begin_shutdown(ShutdownKind::Reboot, controller, observed_at_ns)
            .map(SupervisorShutdownSignalAction::Graceful)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupervisorPid1SignalFdTurn {
    Shutdown(Box<SupervisorShutdownSignalDispatch>),
    Other { signal: i32 },
    WouldBlock,
}
