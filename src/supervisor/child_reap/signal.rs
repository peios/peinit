pub(super) fn signal_failure_cause(signal: i32, core_dumped: bool) -> String {
    if core_dumped {
        format!("ProcessCrash: signal {signal} (core dumped)")
    } else {
        format!("ProcessCrash: signal {signal}")
    }
}
