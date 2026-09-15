use crate::boundary::{Clock, RealtimeClock};
use crate::control::service_security::{ServiceAccess, ServiceAccessChecker};
use crate::control::system::ControlPeer;
use crate::control::wire::{
    ControlCommand, ParsedControlRequest, control_list_response_line,
    control_operation_status_response_line, control_status_response_line,
};
use crate::ids::OperationId;
use crate::supervisor::Supervisor;

use super::time::response_time_projection;
use super::{SupervisorControlCommandBodyError, SupervisorControlCommandBodyResponse};

impl Supervisor {
    pub(super) fn run_control_status_query<C, A>(
        &self,
        parsed: &ParsedControlRequest,
        peer: &ControlPeer,
        access_checker: &mut A,
        clock: &mut C,
    ) -> Result<SupervisorControlCommandBodyResponse, SupervisorControlCommandBodyError>
    where
        C: Clock + RealtimeClock + ?Sized,
        A: ServiceAccessChecker + ?Sized,
    {
        let service = parsed
            .service
            .as_deref()
            .ok_or(SupervisorControlCommandBodyError::InvalidArguments)?;
        self.check_service_command_access(peer, access_checker, service, ControlCommand::Status)?;
        let view = self
            .service_status(service)
            .map_err(SupervisorControlCommandBodyError::Query)?;
        Ok(SupervisorControlCommandBodyResponse::accepted_response(
            control_status_response_line(&view, response_time_projection(clock)?)
                .map_err(SupervisorControlCommandBodyError::serialize)?,
        ))
    }

    pub(super) fn run_control_list_query<A>(
        &self,
        peer: &ControlPeer,
        access_checker: &mut A,
    ) -> Result<SupervisorControlCommandBodyResponse, SupervisorControlCommandBodyError>
    where
        A: ServiceAccessChecker + ?Sized,
    {
        let (services, access_denials) = self.list_authorized_services(peer, access_checker)?;
        Ok(SupervisorControlCommandBodyResponse::Accepted {
            response_line: Some(
                control_list_response_line(&services)
                    .map_err(SupervisorControlCommandBodyError::serialize)?,
            ),
            dispatch: None,
            wait: None,
            access_denials,
            job_access_denials: Vec::new(),
        })
    }

    pub(super) fn run_control_operation_status_query<C, A>(
        &self,
        parsed: &ParsedControlRequest,
        peer: &ControlPeer,
        access_checker: &mut A,
        clock: &mut C,
    ) -> Result<SupervisorControlCommandBodyResponse, SupervisorControlCommandBodyError>
    where
        C: Clock + RealtimeClock + ?Sized,
        A: ServiceAccessChecker + ?Sized,
    {
        let operation_id = parsed
            .operation_id
            .as_deref()
            .ok_or(SupervisorControlCommandBodyError::InvalidArguments)?
            .parse::<OperationId>()
            .map_err(SupervisorControlCommandBodyError::OperationIdParse)?;
        let view = self
            .operation_status(operation_id)
            .map_err(SupervisorControlCommandBodyError::Query)?;
        if self.services.get(&view.service).is_some() {
            self.check_service_access(
                peer,
                access_checker,
                &view.service,
                ServiceAccess::QUERY_STATUS,
            )?;
        } else {
            // The operation is known; only its service is gone (§3.8
            // discarded it once its definition was withdrawn and it
            // drained). A retained operation is meant to be queryable for
            // its retention window, so the right is checked against the
            // descriptor the service had when the operation was created
            // rather than answering UNKNOWN_SERVICE (PEI-1076).
            let descriptor = self
                .operations
                .get(operation_id)
                .and_then(|record| record.service_security.clone())
                .unwrap_or_default();
            self.check_service_access_with_descriptor(
                peer,
                access_checker,
                &view.service,
                &descriptor,
                ServiceAccess::QUERY_STATUS,
            )?;
        }
        Ok(SupervisorControlCommandBodyResponse::accepted_response(
            control_operation_status_response_line(&view, response_time_projection(clock)?)
                .map_err(SupervisorControlCommandBodyError::serialize)?,
        ))
    }
}
