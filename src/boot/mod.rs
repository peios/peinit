pub mod phase2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootMode {
    Full,
    Safe,
    Recovery,
}

impl BootMode {
    /// The mode's name on the control socket (PSPU §4.15).
    pub fn wire(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Safe => "safe",
            Self::Recovery => "recovery",
        }
    }
}

/// Why this boot is in the mode it is in, as far as peinit can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootModeReason {
    /// A Full boot, which is what a boot is unless something says otherwise.
    Normal,
    /// The boot was asked for in this mode: `peios.safemode=1`.
    Requested,
    /// A Full boot that Phase 2 downgraded to Safe, because a Critical
    /// service is in a dependency cycle or an unresolvable conflict (TRM
    /// §2.6).
    SafeModeDowngrade,
}

impl BootModeReason {
    pub fn wire(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Requested => "requested",
            Self::SafeModeDowngrade => "safe_mode_downgrade",
        }
    }
}

/// The boot attempt counter as Phase 1 found it (TRM §2.7), carried to the
/// runtime so `boot` can report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootAttempts {
    /// The count the recovery threshold was checked against: the counter
    /// before this boot's increment, or 0 when the increment could not be
    /// written, which is how Phase 1 treats a counter it cannot advance.
    pub counted: u32,
    /// `peios.bootattempts`, or its default. 0 disables the check.
    pub threshold: u32,
}

impl Default for BootAttempts {
    fn default() -> Self {
        Self {
            counted: 0,
            threshold: crate::init::DEFAULT_BOOT_ATTEMPT_THRESHOLD,
        }
    }
}
