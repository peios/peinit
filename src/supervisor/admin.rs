use crate::boundary::Clock;
use crate::control::lifecycle::LifecycleCommand;
use crate::security::TokenSummary;

use super::dispatch::SupervisorLifecycleDispatch;
use super::state::{Supervisor, SupervisorError};

impl Supervisor {
    pub fn start_service<C>(
        &mut self,
        service: impl Into<String>,
        caller: Option<TokenSummary>,
        clock: &mut C,
    ) -> Result<SupervisorLifecycleDispatch, SupervisorError>
    where
        C: Clock + ?Sized,
    {
        self.run_lifecycle_command(LifecycleCommand::Start, service, caller, clock)
    }

    pub fn stop_service<C>(
        &mut self,
        service: impl Into<String>,
        caller: Option<TokenSummary>,
        clock: &mut C,
    ) -> Result<SupervisorLifecycleDispatch, SupervisorError>
    where
        C: Clock + ?Sized,
    {
        self.run_lifecycle_command(LifecycleCommand::Stop, service, caller, clock)
    }

    pub fn restart_service<C>(
        &mut self,
        service: impl Into<String>,
        caller: Option<TokenSummary>,
        clock: &mut C,
    ) -> Result<SupervisorLifecycleDispatch, SupervisorError>
    where
        C: Clock + ?Sized,
    {
        self.run_lifecycle_command(LifecycleCommand::Restart, service, caller, clock)
    }

    pub fn reload_service<C>(
        &mut self,
        service: impl Into<String>,
        caller: Option<TokenSummary>,
        clock: &mut C,
    ) -> Result<SupervisorLifecycleDispatch, SupervisorError>
    where
        C: Clock + ?Sized,
    {
        self.run_lifecycle_command(LifecycleCommand::Reload, service, caller, clock)
    }

    pub fn reset_service<C>(
        &mut self,
        service: impl Into<String>,
        caller: Option<TokenSummary>,
        clock: &mut C,
    ) -> Result<SupervisorLifecycleDispatch, SupervisorError>
    where
        C: Clock + ?Sized,
    {
        self.run_lifecycle_command(LifecycleCommand::Reset, service, caller, clock)
    }
}
