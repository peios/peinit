use std::collections::{BTreeMap, BTreeSet};

use crate::boot::phase2::{BlockedReason, StartCause};
use crate::service::ServiceDefinition;

pub(super) type ServiceMap<'a> = BTreeMap<&'a str, &'a ServiceDefinition>;
pub(super) type StartableSet = BTreeSet<String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::boot::phase2) struct Phase2BootGraph {
    pub(in crate::boot::phase2) ordered_startable: Vec<StartableService>,
    pub(in crate::boot::phase2) blocked: Vec<BlockedServiceDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::boot::phase2) struct StartableService {
    pub(in crate::boot::phase2) name: String,
    pub(in crate::boot::phase2) cause: StartCause,
    pub(in crate::boot::phase2) identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::boot::phase2) struct BlockedServiceDraft {
    pub(in crate::boot::phase2) service: String,
    pub(in crate::boot::phase2) reason: BlockedReason,
}
