use super::sys::errno;

const LINUX_SIGNAL_UPPER_BOUND: libc::c_int = 65;

pub(super) fn reset_signal_environment() -> Result<(), i32> {
    let mut empty = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
    if unsafe { libc::sigemptyset(empty.as_mut_ptr()) } != 0 {
        return Err(errno());
    }
    if unsafe { libc::sigprocmask(libc::SIG_SETMASK, empty.as_ptr(), std::ptr::null_mut()) } != 0 {
        return Err(errno());
    }

    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = libc::SIG_DFL;
    if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0 {
        return Err(errno());
    }

    for signal in 1..LINUX_SIGNAL_UPPER_BOUND {
        if signal == libc::SIGKILL || signal == libc::SIGSTOP {
            continue;
        }
        if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } != 0 {
            let error = errno();
            if error != libc::EINVAL {
                return Err(error);
            }
        }
    }
    Ok(())
}
