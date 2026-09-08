use std::collections::BTreeSet;

use super::ServiceDefinition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceDependencyKind {
    Requires,
    Wants,
    BindsTo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDependency {
    /// The service depended on. Never carries a level suffix — see
    /// [`split_target`].
    pub target: String,
    /// The readiness level the target must have published, when the
    /// declaration named one. `None` is the ordinary "that service must be
    /// active" dependency.
    pub level: Option<String>,
    pub kind: ServiceDependencyKind,
}

/// Split a declared dependency into a service and an optional level.
///
/// `Requires = ["netd:routed"]` waits for netd to be active *and* to have
/// published `LEVEL=routed`; `Requires = ["netd"]` is unchanged. A level
/// dependency is a service dependency with a stricter predicate, so
/// `netd:routed` subsumes `netd` rather than being a separate kind of
/// edge — which is why this rides on the existing fields rather than a
/// field of its own.
///
/// **The syntax cannot collide with a service name.**
/// [`super::is_valid_service_name`] admits only alphanumerics, `.`, `_`
/// and `-`, so a colon never appears in one and no existing definition can
/// accidentally become a level dependency. Split on the first colon, so a
/// level containing one is still read whole.
pub fn split_target(declared: &str) -> (String, Option<String>) {
    match declared.split_once(':') {
        // An empty level (`"netd:"`) is a typo, not a request for the
        // empty level; treat it as the plain service dependency it looks
        // like rather than a condition nothing can ever satisfy.
        Some((service, level)) if !level.is_empty() => {
            (service.to_string(), Some(level.to_string()))
        }
        Some((service, _)) => (service.to_string(), None),
        None => (declared.to_string(), None),
    }
}

fn dependency(declared: &str, kind: ServiceDependencyKind) -> ServiceDependency {
    let (target, level) = split_target(declared);
    ServiceDependency {
        target,
        level,
        kind,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceDependencyOrderCycle {
    pub services: Vec<String>,
}

pub fn hard_dependencies(definition: &ServiceDefinition) -> Vec<ServiceDependency> {
    definition
        .requires
        .iter()
        .map(|d| dependency(d, ServiceDependencyKind::Requires))
        .chain(
            definition
                .binds_to
                .iter()
                .map(|d| dependency(d, ServiceDependencyKind::BindsTo)),
        )
        .collect()
}

pub fn all_declared_dependencies(definition: &ServiceDefinition) -> Vec<ServiceDependency> {
    definition
        .requires
        .iter()
        .map(|d| dependency(d, ServiceDependencyKind::Requires))
        .chain(
            definition
                .wants
                .iter()
                .map(|d| dependency(d, ServiceDependencyKind::Wants)),
        )
        .chain(
            definition
                .binds_to
                .iter()
                .map(|d| dependency(d, ServiceDependencyKind::BindsTo)),
        )
        .collect()
}

/// The services this one must be ordered after.
///
/// Levels are stripped: ordering is between *services*, and a level is an
/// extra condition on one of them. A graph built from the raw strings
/// would look for a service called `netd:routed` and never find it.
pub fn start_order_dependency_targets(definition: &ServiceDefinition) -> Vec<String> {
    definition
        .requires
        .iter()
        .chain(definition.binds_to.iter())
        .chain(definition.wants.iter())
        .map(|declared| split_target(declared).0)
        .collect()
}

pub fn existing_start_order_dependency_targets(
    definition: &ServiceDefinition,
    contains: impl Fn(&str) -> bool,
) -> Vec<String> {
    let mut seen = BTreeSet::new();
    start_order_dependency_targets(definition)
        .into_iter()
        .filter(|target| contains(target))
        .filter(|target| seen.insert(target.clone()))
        .collect()
}

pub fn dependency_start_order(
    startable: &BTreeSet<String>,
    dependencies_for: impl Fn(&str) -> Vec<String>,
) -> Result<Vec<String>, ServiceDependencyOrderCycle> {
    let mut permanent = BTreeSet::new();
    let mut temporary = Vec::new();
    let mut order = Vec::new();

    for service in startable {
        visit_order(
            service,
            startable,
            &dependencies_for,
            &mut permanent,
            &mut temporary,
            &mut order,
        )?;
    }

    Ok(order)
}

fn visit_order(
    service: &str,
    startable: &BTreeSet<String>,
    dependencies_for: &impl Fn(&str) -> Vec<String>,
    permanent: &mut BTreeSet<String>,
    temporary: &mut Vec<String>,
    order: &mut Vec<String>,
) -> Result<(), ServiceDependencyOrderCycle> {
    if permanent.contains(service) {
        return Ok(());
    }
    if let Some(index) = temporary.iter().position(|entry| entry == service) {
        return Err(ServiceDependencyOrderCycle {
            services: temporary[index..].to_vec(),
        });
    }

    temporary.push(service.to_string());
    for dependency in dependencies_for(service) {
        if startable.contains(&dependency) {
            visit_order(
                &dependency,
                startable,
                dependencies_for,
                permanent,
                temporary,
                order,
            )?;
        }
    }
    temporary.pop();
    permanent.insert(service.to_string());
    order.push(service.to_string());
    Ok(())
}

#[cfg(test)]
mod level_tests {
    use super::*;

    #[test]
    fn a_plain_name_has_no_level() {
        assert_eq!(split_target("netd"), ("netd".into(), None));
        assert_eq!(
            split_target("lpsd-first-account"),
            ("lpsd-first-account".into(), None)
        );
    }

    #[test]
    fn a_level_splits_off() {
        assert_eq!(
            split_target("netd:routed"),
            ("netd".into(), Some("routed".into()))
        );
        assert_eq!(
            split_target("timed:synchronised"),
            ("timed".into(), Some("synchronised".into()))
        );
    }

    #[test]
    fn the_syntax_cannot_collide_with_a_service_name() {
        // The property the whole design rests on: no valid service name
        // contains a colon, so no existing definition can accidentally
        // become a level dependency.
        for name in ["netd", "a.b", "a_b", "a-b", "svc0"] {
            assert!(crate::service::is_valid_service_name(name));
            assert_eq!(split_target(name), (name.to_string(), None));
        }
        assert!(!crate::service::is_valid_service_name("netd:routed"));
    }

    #[test]
    fn only_the_first_colon_splits() {
        // A level is opaque to peinit, so one containing a colon is read
        // whole rather than truncated.
        assert_eq!(split_target("a:b:c"), ("a".into(), Some("b:c".into())));
    }

    #[test]
    fn a_trailing_colon_is_a_typo_not_an_empty_level() {
        // "netd:" reads as a plain dependency on netd. The alternative —
        // a condition matching the empty level — is satisfied by nothing
        // and would hang the dependent for ever on a stray keystroke.
        assert_eq!(split_target("netd:"), ("netd".into(), None));
    }

    #[test]
    fn ordering_targets_never_carry_a_level() {
        // The graph is keyed by service name; a raw string here would send
        // it looking for a service called "netd:routed".
        let mut definition = ServiceDefinition::simple_system_boot("dependent", "/bin/true");
        definition.requires = vec!["netd:routed".into()];
        definition.wants = vec!["timed:synchronised".into()];
        definition.binds_to = vec!["trustd".into()];
        let targets = start_order_dependency_targets(&definition);
        assert!(targets.contains(&"netd".to_string()));
        assert!(targets.contains(&"timed".to_string()));
        assert!(targets.contains(&"trustd".to_string()));
        assert!(targets.iter().all(|t| !t.contains(':')));
    }

    #[test]
    fn a_level_dependency_keeps_its_kind() {
        let mut definition = ServiceDefinition::simple_system_boot("dependent", "/bin/true");
        definition.requires = vec!["netd:routed".into()];
        definition.wants = vec!["timed:synchronised".into()];
        let all = all_declared_dependencies(&definition);
        let requires = all
            .iter()
            .find(|d| d.kind == ServiceDependencyKind::Requires)
            .unwrap();
        assert_eq!(requires.target, "netd");
        assert_eq!(requires.level.as_deref(), Some("routed"));
        let wants = all
            .iter()
            .find(|d| d.kind == ServiceDependencyKind::Wants)
            .unwrap();
        assert_eq!(wants.target, "timed");
        assert_eq!(wants.level.as_deref(), Some("synchronised"));
    }
}
