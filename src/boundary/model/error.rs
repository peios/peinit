use super::process::ProcessLaunchError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryError {
    Registry(String),
    Clock(String),
    Token(String),
    Process(String),
    ProcessLaunch(ProcessLaunchError),
    Timer(String),
    Recovery(String),
    Shutdown(String),
    EventdLog(String),
    Kmes(String),
    /// The kernel refused an event with this error number, kept as a
    /// number so the record of the refusal can carry it
    /// (`peinit.event.dropped`'s `outcome.errno`).
    KmesRefused { errno: i32, message: String },
}

impl BoundaryError {
    /// The error's own words, for a person: the message every variant but
    /// a process launch's carries.
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Registry(text)
            | Self::Clock(text)
            | Self::Token(text)
            | Self::Process(text)
            | Self::Timer(text)
            | Self::Recovery(text)
            | Self::Shutdown(text)
            | Self::EventdLog(text)
            | Self::Kmes(text)
            | Self::KmesRefused { message: text, .. } => Some(text),
            Self::ProcessLaunch(_) => None,
        }
    }

    /// The error number a KMES refusal returned, when it gave one.
    pub fn kmes_errno(&self) -> Option<i32> {
        match self {
            Self::KmesRefused { errno, .. } => Some(*errno),
            _ => None,
        }
    }
}
