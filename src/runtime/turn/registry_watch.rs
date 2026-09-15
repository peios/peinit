use crate::boundary::{RegistryClient, RegistryWatchEventKind, RegistryWatchSource};
use crate::runtime::{RuntimeRegistryWatchTurn, RuntimeShutdownEventTurn};
use crate::supervisor::Supervisor;

use super::model::RuntimeEventRegistrar;

pub(crate) fn process_registry_watch_event<R, G, W>(
    supervisor: &mut Supervisor,
    fd: i32,
    registry: Option<&mut G>,
    watch_source: Option<&mut W>,
    registrar: &mut R,
) -> RuntimeShutdownEventTurn
where
    R: RuntimeEventRegistrar + ?Sized,
    G: RegistryClient,
    W: RegistryWatchSource + ?Sized,
{
    let turn = match (registry, watch_source) {
        (None, _) => RuntimeRegistryWatchTurn::Unavailable {
            fd,
            reason: "runtime registry client unavailable".to_string(),
        },
        (_, None) => RuntimeRegistryWatchTurn::Unavailable {
            fd,
            reason: "runtime registry watch source unavailable".to_string(),
        },
        (Some(registry), Some(watch_source)) => {
            match watch_source.drain_registry_watch_events(fd) {
                Ok(events) if events.is_empty() => RuntimeRegistryWatchTurn::NoEvents { fd },
                Ok(events) => {
                    let overflow = events
                        .iter()
                        .any(|event| event.kind == RegistryWatchEventKind::Overflow);
                    // During the boot window the reload still runs; the
                    // supervisor keeps the not-yet-launched boot-plan members
                    // on the plan's definition and reports them as deferred
                    // (§3.7, PEI-350).
                    let outcome = supervisor.reload_config_from_registry(registry);
                    RuntimeRegistryWatchTurn::ReloadConfig {
                        events,
                        overflow,
                        outcome: Box::new(outcome),
                    }
                }
                Err(error) => {
                    let source_disabled = registrar.unregister_source(fd).is_ok();
                    RuntimeRegistryWatchTurn::ReadFailed {
                        fd,
                        error,
                        source_disabled,
                    }
                }
            }
        }
    };

    RuntimeShutdownEventTurn::RegistryWatch { fd, turn }
}
