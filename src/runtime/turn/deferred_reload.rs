use crate::boundary::RegistryClient;
use crate::runtime::{RuntimeDeferredRegistryReloadTurn, RuntimeShutdownEventTurn};
use crate::supervisor::Supervisor;

/// Run the reload the boot window deferred, if the boot plan has drained.
///
/// Called at both turn boundaries — after the pre-wait work pump and after
/// the post-sources one — because the plan can drain at either: the last
/// boot-plan service goes Active on a notify datagram read as a source, or
/// its launch fails in the pump. Checking only one side would leave the
/// reload waiting on whatever event happened to come next, which on a quiet
/// machine is nothing for a long time (PEI-350).
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
