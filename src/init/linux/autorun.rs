//! Phase-1.5 autorun: run every script in the image's autorun.d, every boot.
//!
//! This is peinit's *generic* first-boot/every-boot hook. peinit knows nothing
//! about what the scripts do — it just runs them. The directory is populated by
//! peiso (the image composer); packages cannot write there (`/usr/system` is
//! package-forbidden), so a package can't slip a boot script in.
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

use std::path::PathBuf;
use std::process::Command;

use crate::boundary::BoundaryError;

use super::recovery_console::write_console;

/// Composer-curated boot scripts. Under `/usr/system` so packages can't write
/// here; peiso places them (e.g. the seed-apply script alongside the seed queue).
const AUTORUN_DIR: &str = "/usr/system/libexec/autorun.d";

/// PATH handed to autorun scripts so they can call system tools (`reg`, `feat`,
/// …) by name — PID 1 itself carries none.
const AUTORUN_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";

pub(super) fn run_autorun_scripts() -> Result<(), BoundaryError> {
    let mut scripts = match list_scripts() {
        Ok(scripts) => scripts,
        Err(error) => {
            let _ = write_console(&format!(
                "peinit warning: could not read autorun dir {AUTORUN_DIR}: {error}\n"
            ));
            return Ok(());
        }
    };
    if scripts.is_empty() {
        let _ = write_console("peinit: no autorun scripts\n");
        return Ok(());
    }
    scripts.sort();

    let mut ran = 0usize;
    for script in &scripts {
        // Invoked by absolute path so the script's `$0` is usable for `rm "$0"`.
        match Command::new(script)
            .current_dir("/")
            .env("PATH", AUTORUN_PATH)
            .status()
        {
            Ok(status) if status.success() => ran += 1,
            Ok(status) => {
                let _ = write_console(&format!(
                    "peinit warning: autorun {} exited {}\n",
                    script.display(),
                    status.code().unwrap_or(-1),
                ));
            }
            Err(error) => {
                let _ = write_console(&format!(
                    "peinit warning: failed to run autorun {}: {error}\n",
                    script.display(),
                ));
            }
        }
    }
    let _ = write_console(&format!("peinit: ran {ran} autorun script(s)\n"));
    Ok(())
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
