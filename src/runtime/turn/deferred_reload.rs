use crate::boundary::RegistryClient;
use crate::runtime::{RuntimeDeferredRegistryReloadTurn, RuntimeShutdownEventTurn};
use crate::supervisor::Supervisor;

/// Apply what the boot window deferred, once it has closed: one plain
/// re-read reload, with nothing frozen any more, so the definitions left
/// pending on boot-plan members land (or the members' removal does).
///
/// Called at both turn boundaries — after the pre-wait work pump and after
/// the post-sources one — because the last planned launch can be attempted
/// at either: in the pump, or released by a notify datagram read as a
/// source. Checking only one side would leave the reload waiting on
/// whatever event happened to come next, which on a quiet machine is
/// nothing for a long time (PEI-350). Nothing runs when nothing was
/// deferred.
///
/// With no registry client there is nothing to re-read from; the deferral
/// stays recorded, which is the truthful state.
pub(crate) fn run_deferred_registry_reload<G>(
    supervisor: &mut Supervisor,
    registry: Option<&mut G>,
) -> Option<RuntimeShutdownEventTurn>
where
    G: RegistryClient,
{
    if !supervisor.has_deferred_registry_reload() {
        return None;
    }
    let registry = registry?;
    let deferred = supervisor.take_due_deferred_registry_reload()?;
    let outcome = supervisor.reload_config_from_registry(registry);
    Some(RuntimeShutdownEventTurn::DeferredRegistryReload {
        turn: RuntimeDeferredRegistryReloadTurn {
            deferred,
            outcome: Box::new(outcome),
        },
    })
}
