use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchdogError {
    InvalidRuntimeUpdate { service: String, value: String },
    UnknownService { service: String },
    MissingMainJob { service: String },
    JobStore(crate::job::JobStoreError),
    ServiceTable(crate::service::ServiceTableError),
    Boundary(crate::boundary::BoundaryError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::supervisor) struct WatchdogStore {
    records: BTreeMap<String, WatchdogRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WatchdogRecord {
    service: String,
    activation_generation: u64,
    cgroup_generation: u64,
    interval_usec: u64,
    due_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchdogDeadline {
    pub service: String,
    pub generation: u64,
    pub cgroup_generation: u64,
    pub due_at_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchdogUpdate {
    Armed { due_at_ns: u64 },
    Disabled,
    Ignored,
}

impl WatchdogStore {
    pub fn new() -> Self {
        Self {
            records: BTreeMap::new(),
        }
    }

    pub fn arm(
        &mut self,
        service: &str,
        activation_generation: u64,
        cgroup_generation: u64,
        interval_usec: u64,
        observed_at_ns: u64,
    ) {
        if interval_usec == 0 {
            self.cancel_service(service);
            return;
        }
        self.records.insert(
            service.to_string(),
            WatchdogRecord {
                service: service.to_string(),
                activation_generation,
                cgroup_generation,
                interval_usec,
                due_at_ns: due_at(observed_at_ns, interval_usec),
            },
        );
    }

    pub fn cancel_service(&mut self, service: &str) {
        self.records.remove(service);
    }

    pub fn reset(&mut self, service: &str, generation: u64, observed_at_ns: u64) -> WatchdogUpdate {
        let Some(record) = self.records.get_mut(service) else {
            return WatchdogUpdate::Ignored;
        };
        if record.activation_generation != generation {
            return WatchdogUpdate::Ignored;
        }
        record.due_at_ns = due_at(observed_at_ns, record.interval_usec);
        WatchdogUpdate::Armed {
            due_at_ns: record.due_at_ns,
        }
    }

    pub fn update_interval(
        &mut self,
        service: &str,
        activation_generation: u64,
        cgroup_generation: u64,
        interval_usec: u64,
        observed_at_ns: u64,
    ) -> WatchdogUpdate {
        if interval_usec == 0 {
            self.cancel_service(service);
            return WatchdogUpdate::Disabled;
        }
        self.arm(
            service,
            activation_generation,
            cgroup_generation,
            interval_usec,
            observed_at_ns,
        );
        WatchdogUpdate::Armed {
            due_at_ns: due_at(observed_at_ns, interval_usec),
        }
    }

    pub fn next_deadline(&self) -> Option<WatchdogDeadline> {
        self.deadlines()
            .min_by_key(|deadline| (deadline.due_at_ns, deadline.service.clone()))
    }

    pub fn due_deadlines(&self, now_ns: u64) -> Vec<WatchdogDeadline> {
        self.deadlines()
            .filter(|deadline| deadline.due_at_ns <= now_ns)
            .collect()
    }

    fn deadlines(&self) -> impl Iterator<Item = WatchdogDeadline> + '_ {
        self.records.values().map(|record| WatchdogDeadline {
            service: record.service.clone(),
            generation: record.activation_generation,
            cgroup_generation: record.cgroup_generation,
            due_at_ns: record.due_at_ns,
        })
    }
}

impl Default for WatchdogStore {
    fn default() -> Self {
        Self::new()
    }
}

fn due_at(observed_at_ns: u64, interval_usec: u64) -> u64 {
    observed_at_ns.saturating_add(interval_usec.saturating_mul(1_000))
}
