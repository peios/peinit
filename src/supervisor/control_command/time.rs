use crate::boundary::{Clock, RealtimeClock};
use crate::control::wire::ControlResponseTimeProjection;
use crate::supervisor::SupervisorError;

use super::SupervisorControlCommandBodyError;

pub(super) fn response_time_projection<C>(
    clock: &mut C,
) -> Result<ControlResponseTimeProjection, SupervisorControlCommandBodyError>
where
    C: Clock + RealtimeClock + ?Sized,
{
    let monotonic_now_ns = clock.monotonic_ns().map_err(|error| {
        SupervisorControlCommandBodyError::supervisor(SupervisorError::Clock(error))
    })?;
    let realtime_now_ns = clock.realtime_ns().map_err(|error| {
        SupervisorControlCommandBodyError::supervisor(SupervisorError::Clock(error))
    })?;
    Ok(ControlResponseTimeProjection::new(
        monotonic_now_ns,
        realtime_now_ns,
    ))
}
