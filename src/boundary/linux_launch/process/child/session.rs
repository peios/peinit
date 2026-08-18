//! Session and controlling-terminal setup for terminal-attached services.
//!
//! A process that merely has a tty on fds 0/1/2 does not *own* that terminal.
//! Without a controlling terminal there is no job control — no Ctrl-C, no
//! `fg`/`bg`, no SIGHUP on hangup — and `login`-style programs that expect to
//! be session leaders misbehave. Both steps below run only for services that
//! declare a `TTYPath`; a daemon keeps peinit's session, which is what the
//! cgroup-based supervision model already assumes.

use super::sys::errno;

/// `setsid()`: leave peinit's session and become the leader of a new one, with
/// no controlling terminal.
///
/// Must precede [`acquire_controlling_terminal`] — `TIOCSCTTY` requires a
/// session leader that does not already own a terminal. Cannot fail with
/// EPERM here in practice: that is returned when the caller is already a
/// process-group leader, and a freshly cloned child never is.
pub(super) fn become_session_leader() -> Result<(), i32> {
    if unsafe { libc::setsid() } < 0 {
        return Err(errno());
    }
    Ok(())
}

/// `ioctl(TIOCSCTTY)`: adopt the terminal already on fd 0 as this session's
/// controlling terminal.
///
/// Called after the tty has been duped onto the standard streams, so fd 0 is
/// the terminal. The `0` argument means "do not steal": if another session
/// already owns this terminal the call fails rather than silently hijacking it
/// — stealing would let a newly started service pull the terminal out from
/// under a running one, and the failure is the more useful outcome.
pub(super) fn acquire_controlling_terminal() -> Result<(), i32> {
    if unsafe { libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) } < 0 {
        return Err(errno());
    }
    Ok(())
}
