mod blocked;
mod modes;
mod success;
mod validation;

use crate::boot::BootMode;
use crate::ids::{JobIdAllocator, OperationIdAllocator};
use crate::service::ServiceDefinition;

use super::{Phase2BootPlan, prepare_phase2_boot_plan};

const OBSERVED_AT_NS: u64 = 1_717_171_717_123_456_789;

fn plan(services: &[ServiceDefinition]) -> Phase2BootPlan {
    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();
    prepare_phase2_boot_plan(
        BootMode::Full,
        services,
        10,
        OBSERVED_AT_NS,
        &mut operations,
        &mut jobs,
    )
    .expect("boot plan")
}
