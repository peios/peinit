//! Phase-1.5 autorun: run every script in the image's autorun.d, every boot.
//!
//! This is peinit's *generic* first-boot/every-boot hook. peinit knows nothing
//! about what the scripts do — it just runs them. The directory is populated by
//! peiso (the image composer); packages cannot write there (`/lcl/policy` is
//! off peipkg's allowlist), so a package can't slip a boot script in.
//!
//! Scripts own their own lifecycle. peinit runs them on EVERY boot and never
//! deletes them, so:
//!   - a persistent script just runs each boot;
//!   - a once-only script ends with `rm "$0"` to remove itself (it is invoked by
//!     absolute path, so `$0` is that path);
//!   - an idempotent script (e.g. the registry-seed apply, which drains its
//!     queue with `reg apply --once-delete`) is a harmless no-op after the first
//!     boot and needs neither.
//!
//! Fail-open: a missing/empty dir or a script that exits non-zero is a console
//! warning, never a boot abort. Scripts run under peinit's (SYSTEM) token; KACS
//! governs what they may do.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::boundary::BoundaryError;

use crate::console_style::{ConsoleTag, relay_lines, render};

use super::recovery_console::write_console;

/// Autorun output goes to the console tagged, like everything else peinit
/// writes. It does not go through `log_console*` because the autorun step
/// deliberately bypasses the quiet policy: a script running this early and
/// going wrong is worth interrupting anything for (see the peinit TRM).
fn say(tag: ConsoleTag, message: &str) {
    let _ = write_console(&render(tag, message));
}

/// Composer-curated boot scripts. Under `/lcl/policy` so packages can't write
/// here; peiso places them (e.g. the seed-apply script alongside the seed queue).
const AUTORUN_DIR: &str = "/lcl/policy/autorun.d";

/// PATH handed to autorun scripts so they can call system tools (`reg`, `feat`,
/// …) by name — PID 1 itself carries none.
const AUTORUN_PATH: &str = "/sbin:/bin";

pub(super) fn run_autorun_scripts() -> Result<(), BoundaryError> {
    let mut scripts = match list_scripts() {
        Ok(scripts) => scripts,
        Err(error) => {
            say(
                ConsoleTag::Warn,
                &format!("peinit warning: could not read autorun dir {AUTORUN_DIR}: {error}\n"),
            );
            return Ok(());
        }
    };
    if scripts.is_empty() {
        say(ConsoleTag::Skip, "peinit: no autorun scripts\n");
        return Ok(());
    }
    scripts.sort();

    let mut ran = 0usize;
    for script in &scripts {
        // Invoked by absolute path so the script's `$0` is usable for `rm "$0"`.
        //
        // Captured rather than inherited: a script that writes straight to the
        // console produces the only untagged lines in a boot, breaking the
        // column for everything around them. peinit relays what the script said
        // under the script's own name and reports the outcome from the exit
        // code, which is the one thing peinit actually knows.
        match run_capturing(script) {
            Ok((status, captured)) => {
                relay_script_output(script, &captured);
                if status.success() {
                    ran += 1;
                } else {
                    say(
                        ConsoleTag::Warn,
                        &format!(
                            "peinit warning: autorun {} exited {}\n",
                            script.display(),
                            status.code().unwrap_or(-1),
                        ),
                    );
                }
            }
            Err(error) => {
                say(
                    ConsoleTag::Warn,
                    &format!(
                        "peinit warning: failed to run autorun {}: {error}\n",
                        script.display(),
                    ),
                );
            }
        }
    }
    say(
        ConsoleTag::Ok,
        &format!("peinit: ran {ran} autorun script(s)\n"),
    );
    Ok(())
}

/// The name a relayed line is attributed to: the script's own file name.
///
/// Not "peinit" — the line is the script's, and attributing it to peinit is how
/// a seed script's failure gets read as an init failure. Not the full path
/// either; the console is narrow and the directory is fixed.
fn relay_component(script: &std::path::Path) -> String {
    script
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "autorun".to_string())
}

/// Relay a script's captured output to the console, tagged.
fn relay_script_output(script: &std::path::Path, captured: &str) {
    for (tag, line) in relay_lines(&relay_component(script), captured) {
        say(tag, &line);
    }
}

/// Run one script with its output captured, and return its exit status.
///
/// stdout and stderr share one pipe, so the lines interleave in the order the
/// script produced them. Two pipes would need concurrent draining to avoid a
/// script blocking on a full stderr buffer while peinit reads stdout; Phase 1
/// has no epoll loop to do that with, and the console cannot show two columns
/// anyway.
///
/// Read to EOF *before* waiting. The other order deadlocks: a script producing
/// more than a pipe buffer would block on write while peinit blocked on wait.
fn run_capturing(script: &std::path::Path) -> std::io::Result<(std::process::ExitStatus, String)> {
    let mut child = Command::new(script)
        .current_dir("/")
        .env("PATH", AUTORUN_PATH)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // `stderr(piped())` then merged by reading both is not possible without a
    // loop, so take stdout as the primary and drain stderr after it closes.
    // Both are drained to EOF before the wait, which is what keeps this from
    // deadlocking.
    let mut captured = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let mut buffer = Vec::new();
        stdout.read_to_end(&mut buffer)?;
        captured.push_str(&String::from_utf8_lossy(&buffer));
    }
    if let Some(mut stderr) = child.stderr.take() {
        let mut buffer = Vec::new();
        stderr.read_to_end(&mut buffer)?;
        captured.push_str(&String::from_utf8_lossy(&buffer));
    }
    let status = child.wait()?;
    Ok((status, captured))
}

/// Regular files in the autorun dir, in sorted (deterministic) order. A missing
/// directory yields an empty list — nothing to run.
fn list_scripts() -> std::io::Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(AUTORUN_DIR) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut scripts = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type().map(|t| !t.is_dir()).unwrap_or(false) {
            scripts.push(entry.path());
        }
    }
    Ok(scripts)
}

#[cfg(test)]
mod tests {
    use super::{AUTORUN_PATH, relay_component};
    use std::path::Path;

    #[test]
    fn autorun_path_uses_stratafs_runtime_views() {
        assert_eq!(AUTORUN_PATH, "/sbin:/bin");
    }

    /// Lines are attributed to the script, not to peinit: a seed script's
    /// failure read as an init failure sends the reader to the wrong place.
    #[test]
    fn lines_are_attributed_to_the_script() {
        assert_eq!(
            relay_component(Path::new("/lcl/policy/autorun.d/10-apply-seeds.sh")),
            "10-apply-seeds.sh"
        );
        assert_eq!(relay_component(Path::new("/")), "autorun");
    }
}
