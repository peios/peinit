use crate::boundary::{BoundaryError, ProcessSignal};

pub(super) fn signal_number(signal: &ProcessSignal) -> Result<libc::c_int, BoundaryError> {
    match signal {
        ProcessSignal::Sigterm => Ok(libc::SIGTERM),
        ProcessSignal::Sigkill => Ok(libc::SIGKILL),
        ProcessSignal::Sighup => Ok(libc::SIGHUP),
        ProcessSignal::Named(name) => named_signal_number(name)
            .ok_or_else(|| BoundaryError::Process(format!("unsupported signal name: {name}"))),
    }
}

fn named_signal_number(name: &str) -> Option<libc::c_int> {
    Some(match name {
        "SIGHUP" => libc::SIGHUP,
        "SIGINT" => libc::SIGINT,
        "SIGQUIT" => libc::SIGQUIT,
        "SIGILL" => libc::SIGILL,
        "SIGTRAP" => libc::SIGTRAP,
        "SIGABRT" => libc::SIGABRT,
        "SIGBUS" => libc::SIGBUS,
        "SIGFPE" => libc::SIGFPE,
        "SIGUSR1" => libc::SIGUSR1,
        "SIGSEGV" => libc::SIGSEGV,
        "SIGUSR2" => libc::SIGUSR2,
        "SIGPIPE" => libc::SIGPIPE,
        "SIGALRM" => libc::SIGALRM,
        "SIGTERM" => libc::SIGTERM,
        "SIGSTKFLT" => libc::SIGSTKFLT,
        "SIGCHLD" => libc::SIGCHLD,
        "SIGCONT" => libc::SIGCONT,
        "SIGTSTP" => libc::SIGTSTP,
        "SIGTTIN" => libc::SIGTTIN,
        "SIGTTOU" => libc::SIGTTOU,
        "SIGURG" => libc::SIGURG,
        "SIGXCPU" => libc::SIGXCPU,
        "SIGXFSZ" => libc::SIGXFSZ,
        "SIGVTALRM" => libc::SIGVTALRM,
        "SIGPROF" => libc::SIGPROF,
        "SIGWINCH" => libc::SIGWINCH,
        "SIGIO" => libc::SIGIO,
        "SIGPWR" => libc::SIGPWR,
        "SIGSYS" => libc::SIGSYS,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::signal_number;
    use crate::boundary::ProcessSignal;

    #[test]
    fn maps_process_signals_to_linux_numbers() {
        assert_eq!(
            signal_number(&ProcessSignal::Sigterm).expect("sigterm"),
            libc::SIGTERM
        );
        assert_eq!(
            signal_number(&ProcessSignal::Sigkill).expect("sigkill"),
            libc::SIGKILL
        );
        assert_eq!(
            signal_number(&ProcessSignal::Sighup).expect("sighup"),
            libc::SIGHUP
        );
        assert_eq!(
            signal_number(&ProcessSignal::Named("SIGUSR1".to_string())).expect("sigusr1"),
            libc::SIGUSR1,
        );
        assert_eq!(
            signal_number(&ProcessSignal::Named("SIGHUP".to_string())).expect("named sighup"),
            libc::SIGHUP,
        );
        assert!(signal_number(&ProcessSignal::Named("SIGBOGUS".to_string())).is_err());
    }
}
