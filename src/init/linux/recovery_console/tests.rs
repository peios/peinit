use std::collections::{BTreeSet, VecDeque};

use super::{
    RecoveryConsoleBoundary, RecoveryLoopLimit, RecoveryShell, run_recovery_console,
    select_recovery_shell,
};
use crate::boundary::BoundaryError;
use crate::init::InitRecoveryReason;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecoveryCall {
    CanExecute(RecoveryShell),
    Spawn(RecoveryShell),
    Wait(u32),
    Log(String),
    Sync,
    Halt,
}

#[derive(Debug)]
struct FakeRecoveryConsole {
    executable: BTreeSet<RecoveryShell>,
    calls: Vec<RecoveryCall>,
    spawn_failures: VecDeque<RecoveryShell>,
    next_pid: u32,
}

impl FakeRecoveryConsole {
    fn new(executable: &[RecoveryShell]) -> Self {
        Self {
            executable: executable.iter().copied().collect(),
            calls: Vec::new(),
            spawn_failures: VecDeque::new(),
            next_pid: 100,
        }
    }

    fn fail_spawn(mut self, shell: RecoveryShell) -> Self {
        self.spawn_failures.push_back(shell);
        self
    }
}

impl RecoveryConsoleBoundary for FakeRecoveryConsole {
    fn can_execute(&mut self, shell: RecoveryShell) -> Result<bool, BoundaryError> {
        self.calls.push(RecoveryCall::CanExecute(shell));
        Ok(self.executable.contains(&shell))
    }

    fn spawn_shell(&mut self, shell: RecoveryShell) -> Result<u32, BoundaryError> {
        self.calls.push(RecoveryCall::Spawn(shell));
        if self.spawn_failures.front() == Some(&shell) {
            self.spawn_failures.pop_front();
            return Err(BoundaryError::Recovery(format!(
                "spawn {} failed",
                shell.path()
            )));
        }
        let pid = self.next_pid;
        self.next_pid += 1;
        Ok(pid)
    }

    fn wait_for_shell(&mut self, pid: u32) -> Result<(), BoundaryError> {
        self.calls.push(RecoveryCall::Wait(pid));
        Ok(())
    }

    fn log_console(&mut self, message: &str) -> Result<(), BoundaryError> {
        self.calls.push(RecoveryCall::Log(message.to_string()));
        Ok(())
    }

    fn sync_filesystems(&mut self) -> Result<(), BoundaryError> {
        self.calls.push(RecoveryCall::Sync);
        Ok(())
    }

    fn halt(&mut self) -> Result<(), BoundaryError> {
        self.calls.push(RecoveryCall::Halt);
        Ok(())
    }
}

fn reason() -> InitRecoveryReason {
    InitRecoveryReason::ForcedByKernelCommandLine
}

#[test]
fn selects_recsh_when_available() {
    let mut console = FakeRecoveryConsole::new(&[RecoveryShell::Recsh, RecoveryShell::Sh]);

    assert_eq!(
        select_recovery_shell(&mut console).expect("selection"),
        Some(RecoveryShell::Recsh)
    );
}

#[test]
fn falls_back_to_sh_when_recsh_is_not_executable() {
    let mut console = FakeRecoveryConsole::new(&[RecoveryShell::Sh]);

    assert_eq!(
        select_recovery_shell(&mut console).expect("selection"),
        Some(RecoveryShell::Sh)
    );
}

#[test]
fn respawns_shell_after_exit() {
    let mut console = FakeRecoveryConsole::new(&[RecoveryShell::Sh]);

    run_recovery_console(&mut console, &reason(), RecoveryLoopLimit::Sessions(2))
        .expect("bounded recovery loop");

    assert_eq!(
        console.calls,
        vec![
            RecoveryCall::Log(
                "peinit entering Recovery mode: ForcedByKernelCommandLine\n".to_string()
            ),
            RecoveryCall::CanExecute(RecoveryShell::Recsh),
            RecoveryCall::CanExecute(RecoveryShell::Sh),
            RecoveryCall::Spawn(RecoveryShell::Sh),
            RecoveryCall::Wait(100),
            RecoveryCall::CanExecute(RecoveryShell::Recsh),
            RecoveryCall::CanExecute(RecoveryShell::Sh),
            RecoveryCall::Spawn(RecoveryShell::Sh),
            RecoveryCall::Wait(101),
        ]
    );
}

#[test]
fn spawn_failure_for_recsh_falls_back_to_sh() {
    let mut console = FakeRecoveryConsole::new(&[RecoveryShell::Recsh, RecoveryShell::Sh])
        .fail_spawn(RecoveryShell::Recsh);

    run_recovery_console(&mut console, &reason(), RecoveryLoopLimit::Sessions(1))
        .expect("bounded recovery loop");

    assert!(
        console
            .calls
            .contains(&RecoveryCall::Spawn(RecoveryShell::Recsh))
    );
    assert!(
        console
            .calls
            .contains(&RecoveryCall::Spawn(RecoveryShell::Sh))
    );
    assert!(console.calls.contains(&RecoveryCall::Wait(100)));
}

#[test]
fn missing_shell_logs_syncs_and_halts() {
    let mut console = FakeRecoveryConsole::new(&[]);

    let error = run_recovery_console(&mut console, &reason(), RecoveryLoopLimit::Sessions(1))
        .expect_err("missing shell");

    assert!(format!("{error:?}").contains("halt returned after recovery shell failure"));
    assert!(
        console
            .calls
            .iter()
            .any(|call| matches!(call, RecoveryCall::Sync))
    );
    assert!(
        console
            .calls
            .iter()
            .any(|call| matches!(call, RecoveryCall::Halt))
    );
    assert!(console.calls.iter().any(|call| {
        matches!(call, RecoveryCall::Log(message) if message.contains("neither /bin/recsh nor /bin/sh"))
    }));
}
