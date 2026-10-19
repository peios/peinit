//! Capture boot-script output through one pipe without reader threads in PID 1.

use std::io::{Read, pipe};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

/// Drain the shared stream before waiting, so either output can exceed pipe capacity.
pub(super) fn run_capturing(
    script: &Path,
    search_path: &str,
) -> std::io::Result<(ExitStatus, String)> {
    let (reader, writer) = pipe()?;
    let child = Command::new(script)
        .current_dir("/")
        .env("PATH", search_path)
        .stdin(Stdio::null())
        .stdout(writer.try_clone()?)
        .stderr(writer)
        .spawn()?;
    capture_child(child, reader)
}

fn capture_child(mut child: Child, mut reader: impl Read) -> std::io::Result<(ExitStatus, String)> {
    let mut output = Vec::new();
    if let Err(error) = reader.read_to_end(&mut output) {
        // The child may still be blocked writing output that can no longer be read.
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let status = child.wait()?;
    Ok((status, String::from_utf8_lossy(&output).into_owned()))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Error, ErrorKind, Read};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::{capture_child, run_capturing};

    #[test]
    fn capture_probe() {
        let Some(script) = std::env::var_os("PEINIT_CAPTURE_PROBE_SCRIPT") else {
            return;
        };
        let (status, text) = run_capturing(std::path::Path::new(&script), "/sbin:/bin")
            .expect("capture boot-script output");
        assert!(status.success());
        assert_eq!(text, "0123456789abcdef".repeat(8192) + "firstsecondthird");
    }

    #[test]
    fn large_stderr_and_mixed_output_complete_in_write_order() {
        let directory = std::env::temp_dir().join(format!("peinit-capture-{}", std::process::id()));
        fs::create_dir(&directory).expect("create isolated script directory");
        let script = directory.join("script");
        let contents = r#"#!/bin/sh
i=0
while [ "$i" -lt 8192 ]; do
    printf 0123456789abcdef >&2
    i=$((i+1))
done
printf first
printf second >&2
printf third
"#;
        fs::write(&script, contents).expect("write script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).expect("executable script");
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "init::autorun_process::tests::capture_probe",
                "--nocapture",
            ])
            .env("PEINIT_CAPTURE_PROBE_SCRIPT", &script)
            .stdout(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("start bounded regression probe");
        let deadline = Instant::now() + Duration::from_secs(15);
        let result = loop {
            if let Some(status) = child.try_wait().expect("poll regression probe") {
                break Some(status);
            }
            if Instant::now() >= deadline {
                // SAFETY: the live unreaped probe owns this isolated process group.
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                child.wait().expect("reap timed-out probe");
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        fs::remove_dir_all(directory).expect("remove script fixture");
        assert!(
            result.is_some_and(|status| status.success()),
            "output capture blocked or failed"
        );
    }

    struct FailedReader;

    impl Read for FailedReader {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(Error::new(ErrorKind::BrokenPipe, "injected read failure"))
        }
    }

    #[test]
    fn read_failure_kills_and_reaps_child() {
        let child = Command::new("/bin/sh")
            .args(["-c", "while :; do :; done"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start child");
        let pid = child.id() as libc::pid_t;
        let error = capture_child(child, FailedReader).expect_err("read must fail");
        assert_eq!(error.kind(), ErrorKind::BrokenPipe);
        // SAFETY: a null status pointer is permitted; this only checks child reaping.
        assert_eq!(
            unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) },
            -1
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
    }
}
