use std::fmt;

use crate::boundary::{ProcessPreExecError, ProcessPreExecStep};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ChildSetupEvidence {
    pub step: ProcessPreExecStep,
    pub errno: i32,
}

impl ChildSetupEvidence {
    pub(super) const BYTE_LEN: usize = 8;

    pub(super) fn encode(&self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0u8; Self::BYTE_LEN];
        bytes[..4].copy_from_slice(&self.step.id().to_le_bytes());
        bytes[4..].copy_from_slice(&self.errno.to_le_bytes());
        bytes
    }

    pub(super) fn decode(bytes: [u8; Self::BYTE_LEN]) -> Result<Self, MalformedChildSetupEvidence> {
        let mut step_bytes = [0u8; 4];
        step_bytes.copy_from_slice(&bytes[..4]);
        let mut errno_bytes = [0u8; 4];
        errno_bytes.copy_from_slice(&bytes[4..]);
        let step_id = u32::from_le_bytes(step_bytes);
        let errno = i32::from_le_bytes(errno_bytes);
        let Some(step) = ProcessPreExecStep::from_id(step_id) else {
            return Err(MalformedChildSetupEvidence::UnknownStep { step_id });
        };
        if errno <= 0 {
            return Err(MalformedChildSetupEvidence::InvalidErrno { errno });
        }
        Ok(Self { step, errno })
    }
}

impl fmt::Display for ChildSetupEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "step {} ({}) failed with errno {} ({})",
            self.step.id(),
            self.step.label(),
            self.errno,
            std::io::Error::from_raw_os_error(self.errno),
        )
    }
}

impl From<ChildSetupEvidence> for ProcessPreExecError {
    fn from(evidence: ChildSetupEvidence) -> Self {
        Self {
            step: evidence.step,
            errno: evidence.errno,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ChildSetupStatus {
    Pending,
    ExecSucceeded,
    SetupFailed(ChildSetupEvidence),
    MalformedSetupEvidence(MalformedChildSetupEvidence),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MalformedChildSetupEvidence {
    InvalidLength { len: usize },
    UnknownStep { step_id: u32 },
    InvalidErrno { errno: i32 },
}

impl fmt::Display for MalformedChildSetupEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength { len } => {
                write!(f, "expected 8 bytes, got {len}")
            }
            Self::UnknownStep { step_id } => {
                write!(f, "unknown child setup step {step_id}")
            }
            Self::InvalidErrno { errno } => {
                write!(f, "invalid errno {errno}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::boundary::ProcessPreExecStep;

    use super::{ChildSetupEvidence, MalformedChildSetupEvidence};

    #[test]
    fn child_setup_evidence_round_trips_exact_payload() {
        let evidence = ChildSetupEvidence {
            step: ProcessPreExecStep::SetWorkingDirectory,
            errno: libc::ENOENT,
        };

        assert_eq!(
            ChildSetupEvidence::decode(evidence.encode()).expect("decode"),
            evidence,
        );
    }

    #[test]
    fn child_setup_evidence_rejects_unknown_step_and_invalid_errno() {
        let mut unknown_step = [0u8; ChildSetupEvidence::BYTE_LEN];
        unknown_step[..4].copy_from_slice(&99u32.to_le_bytes());
        unknown_step[4..].copy_from_slice(&libc::EIO.to_le_bytes());
        assert_eq!(
            ChildSetupEvidence::decode(unknown_step),
            Err(MalformedChildSetupEvidence::UnknownStep { step_id: 99 }),
        );

        let mut invalid_errno = [0u8; ChildSetupEvidence::BYTE_LEN];
        invalid_errno[..4].copy_from_slice(&ProcessPreExecStep::Exec.id().to_le_bytes());
        invalid_errno[4..].copy_from_slice(&0i32.to_le_bytes());
        assert_eq!(
            ChildSetupEvidence::decode(invalid_errno),
            Err(MalformedChildSetupEvidence::InvalidErrno { errno: 0 }),
        );
    }
}
