use crate::boundary::ProcessSignal;

use super::*;

#[test]
fn signal_reload_still_sends_sighup_for_absent_exec_reload() {
    let mut supervisor = super::super::active_app_supervisor();
    reload_app(&mut supervisor);
    let mut controller = TestProcessController::default();
    let mut clock = ScriptedClock::new([CONTROL_NS]);

    supervisor
        .execute_next_pending_control_operation(&mut controller, &mut clock)
        .expect("execute reload")
        .expect("reload signal dispatch");

    assert_eq!(controller.signals[0].signal, ProcessSignal::Sighup);
}
