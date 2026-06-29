#[cfg(all(feature = "peios-boundary", feature = "peios-registry"))]
fn main() {
    match peinit2::init::run_linux_peinit() {
        Ok(peinit2::init::InitRunResult::RuntimeReturned) => {
            eprintln!("peinit2 runtime returned unexpectedly");
            std::process::exit(1);
        }
        Ok(peinit2::init::InitRunResult::RecoveryReturned { reason }) => {
            eprintln!("peinit2 recovery returned unexpectedly: {reason:?}");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("peinit2 failed: {error:?}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(all(feature = "peios-boundary", feature = "peios-registry")))]
fn main() {
    eprintln!(
        "peinit2 binary requires features `peios-boundary,peios-registry` for the Linux PID1 path"
    );
    std::process::exit(70);
}
