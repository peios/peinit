use super::process::ProcessLaunchError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryError {
    Registry(String),
    Clock(String),
    Token(String),
    Process(String),
    ProcessLaunch(ProcessLaunchError),
    Timer(String),
    Jfs(String),
    Recovery(String),
    Shutdown(String),
    EventdLog(String),
    Kmes(String),
}
