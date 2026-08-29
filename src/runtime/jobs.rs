//! The jobs channel as the runtime sees it: one object holding the listener
//! and the connection table, behind a trait so the event loop is written
//! once and a test can run it with no channel at all.

use crate::boundary::{Clock, JobIdentityProvider, ProcessController, RealtimeClock};
use crate::control::wire::ControlResponseTimeProjection;
use crate::jobs::connection::{
    JobsAcceptedConnection, JobsConnectionAcceptTurn, JobsConnectionTable, JobsListener,
    accept_jobs_connection_at,
};
use crate::jobs::socket::JobsSocketLimits;
use crate::submitted::{JobAccessChecker, JobDescriptorFactory};
use crate::supervisor::{
    Supervisor, SupervisorJobsConnectionTurn, SupervisorJobsConnectionTurnContext,
    SupervisorJobsConnectionTurnError, SupervisorJobsWaitFlushError, SupervisorJobsWaitFlushTurn,
};

use super::RuntimeEventSource;
use super::turn::{RuntimeEventRegistrar, RuntimeEventRegistrationError};

/// Both halves of job security in one object: the boundary that mints
/// descriptors and the one that evaluates them are the same on Peios.
pub trait JobSecurityBoundary: JobDescriptorFactory + JobAccessChecker {}

impl<T> JobSecurityBoundary for T where T: JobDescriptorFactory + JobAccessChecker + ?Sized {}

pub trait RuntimeClock: Clock + RealtimeClock {}

impl<T> RuntimeClock for T where T: Clock + RealtimeClock + ?Sized {}

/// What a jobs connection turn needs from the loop context.
pub struct RuntimeJobsContext<'a> {
    pub identity_provider: &'a mut dyn JobIdentityProvider,
    pub security: &'a mut dyn JobSecurityBoundary,
    pub controller: &'a mut dyn ProcessController,
    pub clock: &'a mut dyn RuntimeClock,
    pub limits: JobsSocketLimits,
    pub observed_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeJobsConnectionTurn {
    Processed {
        supervisor: Box<SupervisorJobsConnectionTurn>,
        removed: bool,
        active_connections: usize,
    },
    Stale,
}

#[derive(Debug)]
pub enum RuntimeJobsChannelError {
    Accept(crate::jobs::connection::JobsConnectionAcceptError),
    Registration(RuntimeEventRegistrationError),
    Connection(SupervisorJobsConnectionTurnError),
    WaitFlush(SupervisorJobsWaitFlushError),
}

pub trait RuntimeJobsChannel {
    fn accept_jobs_connection(
        &mut self,
        registrar: &mut dyn RuntimeEventRegistrar,
        accepted_at_ns: u64,
    ) -> Result<(JobsConnectionAcceptTurn, Option<RuntimeEventSource>), RuntimeJobsChannelError>;

    fn process_jobs_connection(
        &mut self,
        fd: i32,
        supervisor: &mut Supervisor,
        registrar: &mut dyn RuntimeEventRegistrar,
        context: RuntimeJobsContext<'_>,
    ) -> Result<RuntimeJobsConnectionTurn, RuntimeJobsChannelError>;

    fn flush_jobs_waits(
        &mut self,
        supervisor: &Supervisor,
        time: ControlResponseTimeProjection,
        observed_at_ns: u64,
    ) -> Result<SupervisorJobsWaitFlushTurn, RuntimeJobsChannelError>;

    fn has_pending_jobs_waits(&self) -> bool;

    fn close_idle_jobs_connections(
        &mut self,
        registrar: &mut dyn RuntimeEventRegistrar,
        now_ns: u64,
        timeout_secs: u64,
    ) -> Vec<i32>;

    fn next_jobs_idle_deadline_ns(&self, timeout_secs: u64) -> Option<u64>;

    fn set_max_jobs_connections(&mut self, max_connections: usize);
}

/// The channel with a listener and its table of accepted connections.
#[derive(Debug)]
pub struct RuntimeJobsChannelTable<L>
where
    L: JobsListener,
{
    listener: L,
    connections: JobsConnectionTable<L::Connection>,
}

impl<L> RuntimeJobsChannelTable<L>
where
    L: JobsListener,
{
    pub fn new(listener: L, max_connections: usize) -> Self {
        Self {
            listener,
            connections: JobsConnectionTable::new(max_connections),
        }
    }

    pub fn listener(&self) -> &L {
        &self.listener
    }

    pub fn active_connections(&self) -> usize {
        self.connections.len()
    }
}

impl<L> RuntimeJobsChannel for RuntimeJobsChannelTable<L>
where
    L: JobsListener,
    L::Connection: JobsAcceptedConnection,
{
    fn accept_jobs_connection(
        &mut self,
        registrar: &mut dyn RuntimeEventRegistrar,
        accepted_at_ns: u64,
    ) -> Result<(JobsConnectionAcceptTurn, Option<RuntimeEventSource>), RuntimeJobsChannelError>
    {
        let accept = accept_jobs_connection_at(
            &mut self.listener,
            &mut self.connections,
            Some(accepted_at_ns),
        )
        .map_err(RuntimeJobsChannelError::Accept)?;
        let registration = match &accept {
            JobsConnectionAcceptTurn::Accepted { fd, .. } => {
                let fd = *fd;
                let source = RuntimeEventSource::jobs_connection(fd).map_err(|error| {
                    RuntimeJobsChannelError::Registration(RuntimeEventRegistrationError::Register {
                        fd,
                        source: RuntimeEventSource::JobsListener,
                        message: format!("{error:?}"),
                    })
                })?;
                if let Err(error) = registrar.register_source(fd, source) {
                    self.connections.remove(fd);
                    return Err(RuntimeJobsChannelError::Registration(error));
                }
                Some(source)
            }
            JobsConnectionAcceptTurn::RejectedAtSocket { .. }
            | JobsConnectionAcceptTurn::PeerRejected { .. }
            | JobsConnectionAcceptTurn::WouldBlock => None,
        };
        Ok((accept, registration))
    }

    fn process_jobs_connection(
        &mut self,
        fd: i32,
        supervisor: &mut Supervisor,
        registrar: &mut dyn RuntimeEventRegistrar,
        context: RuntimeJobsContext<'_>,
    ) -> Result<RuntimeJobsConnectionTurn, RuntimeJobsChannelError> {
        let Some(connection) = self.connections.get_mut(fd) else {
            return Ok(RuntimeJobsConnectionTurn::Stale);
        };
        let turn = supervisor
            .process_jobs_connection_turn(
                connection,
                SupervisorJobsConnectionTurnContext {
                    identity_provider: context.identity_provider,
                    security: context.security,
                    controller: context.controller,
                    clock: context.clock,
                    max_message_bytes: context.limits.max_message_bytes,
                    max_descriptors: crate::jobs::socket::MAX_JOBS_MESSAGE_DESCRIPTORS,
                    observed_at_ns: context.observed_at_ns,
                },
            )
            .map_err(RuntimeJobsChannelError::Connection)?;
        let removed = if turn.close_connection {
            let _ = registrar.unregister_source(fd);
            self.connections.remove(fd).is_some()
        } else {
            false
        };
        Ok(RuntimeJobsConnectionTurn::Processed {
            supervisor: Box::new(turn),
            removed,
            active_connections: self.connections.len(),
        })
    }

    fn flush_jobs_waits(
        &mut self,
        supervisor: &Supervisor,
        time: ControlResponseTimeProjection,
        observed_at_ns: u64,
    ) -> Result<SupervisorJobsWaitFlushTurn, RuntimeJobsChannelError> {
        supervisor
            .flush_jobs_waits(&mut self.connections, time, observed_at_ns)
            .map_err(RuntimeJobsChannelError::WaitFlush)
    }

    fn has_pending_jobs_waits(&self) -> bool {
        self.connections.has_pending_jobs_waits()
    }

    fn close_idle_jobs_connections(
        &mut self,
        registrar: &mut dyn RuntimeEventRegistrar,
        now_ns: u64,
        timeout_secs: u64,
    ) -> Vec<i32> {
        let fds = self.connections.jobs_idle_fds(now_ns, timeout_secs);
        for fd in &fds {
            let _ = registrar.unregister_source(*fd);
            self.connections.remove(*fd);
        }
        fds
    }

    fn next_jobs_idle_deadline_ns(&self, timeout_secs: u64) -> Option<u64> {
        self.connections.next_jobs_idle_deadline_ns(timeout_secs)
    }

    fn set_max_jobs_connections(&mut self, max_connections: usize) {
        self.connections.set_max_connections(max_connections);
    }
}

/// A runtime with no jobs channel — the test harnesses that drive the loop
/// without a socket.
#[derive(Debug, Default)]
pub struct NoJobsChannel;

impl RuntimeJobsChannel for NoJobsChannel {
    fn accept_jobs_connection(
        &mut self,
        _registrar: &mut dyn RuntimeEventRegistrar,
        _accepted_at_ns: u64,
    ) -> Result<(JobsConnectionAcceptTurn, Option<RuntimeEventSource>), RuntimeJobsChannelError>
    {
        Ok((JobsConnectionAcceptTurn::WouldBlock, None))
    }

    fn process_jobs_connection(
        &mut self,
        _fd: i32,
        _supervisor: &mut Supervisor,
        _registrar: &mut dyn RuntimeEventRegistrar,
        _context: RuntimeJobsContext<'_>,
    ) -> Result<RuntimeJobsConnectionTurn, RuntimeJobsChannelError> {
        Ok(RuntimeJobsConnectionTurn::Stale)
    }

    fn flush_jobs_waits(
        &mut self,
        _supervisor: &Supervisor,
        _time: ControlResponseTimeProjection,
        _observed_at_ns: u64,
    ) -> Result<SupervisorJobsWaitFlushTurn, RuntimeJobsChannelError> {
        Ok(SupervisorJobsWaitFlushTurn {
            completed: Vec::new(),
        })
    }

    fn has_pending_jobs_waits(&self) -> bool {
        false
    }

    fn close_idle_jobs_connections(
        &mut self,
        _registrar: &mut dyn RuntimeEventRegistrar,
        _now_ns: u64,
        _timeout_secs: u64,
    ) -> Vec<i32> {
        Vec::new()
    }

    fn next_jobs_idle_deadline_ns(&self, _timeout_secs: u64) -> Option<u64> {
        None
    }

    fn set_max_jobs_connections(&mut self, _max_connections: usize) {}
}

/// A runtime with no way to prepare a job identity — every submission is
/// refused as an internal error. For the harnesses that never submit.
#[derive(Debug, Default)]
pub struct NoJobIdentityProvider;

impl crate::boundary::JobIdentityProvider for NoJobIdentityProvider {
    fn prepare_job_identity(
        &mut self,
        _source: crate::boundary::JobIdentitySource,
    ) -> Result<crate::boundary::PreparedJobIdentity, crate::boundary::JobIdentityError> {
        Err(crate::boundary::JobIdentityError::Boundary(
            "job identity preparation unavailable".to_string(),
        ))
    }
}
