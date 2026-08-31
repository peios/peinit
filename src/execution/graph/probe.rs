use crate::service::ServiceTable;

use super::model::LevelProbe;

/// Answer a level edge's question from the live service table.
///
/// Exact match on the level, deliberately: peinit has no ordering over
/// another daemon's vocabulary — see `ServiceEntry::satisfies` for the
/// full argument.
pub fn probe_level(services: &ServiceTable, target: &str, level: &str) -> LevelProbe {
    let Some(entry) = services.get(target) else {
        return LevelProbe::Absent;
    };
    if !entry.runtime.state.satisfies_dependents() {
        return LevelProbe::Absent;
    }
    if entry.runtime.level.as_deref() == Some(level) {
        LevelProbe::Satisfied
    } else {
        LevelProbe::NotYetPublished
    }
}
