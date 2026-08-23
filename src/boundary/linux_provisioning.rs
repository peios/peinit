use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

#[cfg(feature = "peios-boundary")]
use peios::file::{CreateOptions, Disposition, FileAccess, OpenFlags, OpenOptions, SecInfo};

use crate::provisioning::{
    ProvisionedPath, ProvisionedPathApplyFailure, ProvisionedPathApplyReport, ProvisionedPathKind,
    ProvisionedPathSecurity,
};
use crate::security::service_sid;
use crate::service::ServiceDefinition;

#[cfg(feature = "peios-boundary")]
const DEFAULT_PROVISIONED_PATH_SDDL: &str = "O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;FR;;;BU)";

/// Every KACS-native open has to name a data right -- `READ_DATA`,
/// `WRITE_DATA`, `APPEND_DATA` -- or `EXECUTE`, because that is what gives the
/// returned descriptor a file mode; a mask of metadata and standard rights
/// alone fails with `EINVAL`.
///
/// For a directory the only one available is `LIST_DIRECTORY`, which is
/// `READ_DATA` under another name. Its write aliases `ADD_FILE` and
/// `ADD_SUBDIRECTORY` are not cacheable directory handle rights and are
/// rejected with `EOPNOTSUPP` -- they authorize namespace operations on the
/// parent, evaluated live, rather than being carried by a handle.
#[cfg(feature = "peios-boundary")]
const DIRECTORY_CREATE_ACCESS: FileAccess = FileAccess::LIST_DIRECTORY
    .union(FileAccess::READ_ATTRIBUTES)
    .union(FileAccess::SYNCHRONIZE);

#[cfg(feature = "peios-boundary")]
const FILE_CREATE_ACCESS: FileAccess = FileAccess::READ_DATA
    .union(FileAccess::WRITE_DATA)
    .union(FileAccess::READ_ATTRIBUTES)
    .union(FileAccess::SYNCHRONIZE);

pub fn provision_linux_boot_paths(paths: &[ProvisionedPath]) -> ProvisionedPathApplyReport {
    let mut report = ProvisionedPathApplyReport::default();
    for entry in paths {
        match apply_provisioned_path(entry) {
            Ok(()) => report.applied.push(entry.name.clone()),
            Err(error) => {
                let failure = ProvisionedPathApplyFailure {
                    entry: entry.name.clone(),
                    path: entry.path.clone(),
                    message: error.to_string(),
                };
                if entry.required {
                    report.required_failures.push(failure);
                } else {
                    report.warnings.push(failure);
                }
            }
        }
    }
    report
}

pub fn provision_linux_service_runtime_directories(
    service: &ServiceDefinition,
) -> Result<(), io::Error> {
    for directory in &service.runtime_directories {
        let path = PathBuf::from("/run").join(&directory.name);
        // Name the step and the path. A bare errno here reaches the operator as
        // "provision runtime directories for <svc> failed: Invalid argument"
        // with three candidate syscalls behind it, which costs a boot to narrow.
        let sd = service_runtime_directory_security(service)
            .map_err(|error| step_error("build security descriptor for", &path, error))?;
        ensure_directory(&path, &sd)?;
    }
    Ok(())
}

/// Wrap a step failure with what was being attempted and to what.
fn step_error(step: &str, path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{step} {}: {error}", path.display()))
}

fn apply_provisioned_path(entry: &ProvisionedPath) -> io::Result<()> {
    let path = Path::new(&entry.path);
    let sd = provisioned_path_security(&entry.security)?;
    match entry.kind {
        ProvisionedPathKind::Directory => ensure_directory(path, &sd),
        ProvisionedPathKind::File => ensure_file(path, &sd),
    }
}

#[derive(Debug, Clone)]
enum ResolvedSecurity {
    #[cfg(feature = "peios-boundary")]
    Descriptor(peios::security::SecurityDescriptor),
    #[cfg(not(feature = "peios-boundary"))]
    Noop,
}

fn provisioned_path_security(security: &ProvisionedPathSecurity) -> io::Result<ResolvedSecurity> {
    match security {
        ProvisionedPathSecurity::Default => default_provisioned_path_security(),
        ProvisionedPathSecurity::RegistryBinary(bytes) => security_from_registry_bytes(bytes),
    }
}

fn service_runtime_directory_security(service: &ServiceDefinition) -> io::Result<ResolvedSecurity> {
    service_runtime_directory_security_for_sid(&service_sid(&service.name))
}

#[cfg(feature = "peios-boundary")]
fn default_provisioned_path_security() -> io::Result<ResolvedSecurity> {
    peios::security::sddl::parse(DEFAULT_PROVISIONED_PATH_SDDL)
        .map(ResolvedSecurity::Descriptor)
        .map_err(io::Error::from)
}

#[cfg(not(feature = "peios-boundary"))]
fn default_provisioned_path_security() -> io::Result<ResolvedSecurity> {
    Ok(ResolvedSecurity::Noop)
}

#[cfg(feature = "peios-boundary")]
fn service_runtime_directory_security_for_sid(sid: &str) -> io::Result<ResolvedSecurity> {
    peios::security::sddl::parse(&format!(
        "O:SYG:SYD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{sid})"
    ))
    .map(ResolvedSecurity::Descriptor)
    .map_err(io::Error::from)
}

#[cfg(not(feature = "peios-boundary"))]
fn service_runtime_directory_security_for_sid(_sid: &str) -> io::Result<ResolvedSecurity> {
    Ok(ResolvedSecurity::Noop)
}

#[cfg(feature = "peios-boundary")]
fn security_from_registry_bytes(bytes: &[u8]) -> io::Result<ResolvedSecurity> {
    peios::security::SecurityDescriptor::from_validated_bytes(bytes.to_vec())
        .map(ResolvedSecurity::Descriptor)
        .map_err(io::Error::from)
}

#[cfg(not(feature = "peios-boundary"))]
fn security_from_registry_bytes(_bytes: &[u8]) -> io::Result<ResolvedSecurity> {
    Ok(ResolvedSecurity::Noop)
}

fn ensure_directory(path: &Path, sd: &ResolvedSecurity) -> io::Result<()> {
    ensure_parent_exists(path)?;
    let (directory, created) =
        create_or_open_directory(path, sd).map_err(|e| step_error("create directory", path, e))?;
    if created {
        // The creator descriptor was applied atomically at create time, which
        // is the whole point of supplying one. Re-stamping it would be a
        // no-op with a window in front of it.
        return Ok(());
    }
    apply_fd_security(&directory, sd)
        .map_err(|e| step_error("apply security descriptor to directory", path, e))
}

fn ensure_file(path: &Path, sd: &ResolvedSecurity) -> io::Result<()> {
    ensure_parent_exists(path)?;
    let (file, created) =
        create_or_open_file(path, sd).map_err(|e| step_error("create file", path, e))?;
    if !fd_is_regular_file(file.as_raw_fd())? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} exists but is not a regular file", path.display()),
        ));
    }
    if created {
        return Ok(());
    }
    apply_fd_security(&file, sd)
        .map_err(|e| step_error("apply security descriptor to file", path, e))
}

fn ensure_parent_exists(path: &Path) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path has no parent: {}", path.display()),
        ));
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    // Wrapped, not propagated bare: the common failure here is a missing parent
    // (peinit deliberately does not create ancestors), and a raw ENOENT gives an
    // operator no clue which path was missing or that a parent was the issue.
    let metadata = parent
        .symlink_metadata()
        .map_err(|error| step_error("stat parent of", path, error))?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("parent is not a directory: {}", parent.display()),
        ))
    }
}

/// Create `path` carrying `sd`, or open it if something is already there.
///
/// Returns the handle and whether this call created it.
///
/// **Two dispositions, not one `FILE_OPEN_IF`.** A creator descriptor is only
/// meaningful on a branch that creates: KACS rejects `FILE_OPEN_IF` carrying
/// one when it resolves to an existing object, with `EINVAL`, deliberately --
/// there is nothing for the descriptor to apply to and silently dropping it
/// would be worse. So creation and the already-there case are separate opens,
/// and the second applies the descriptor afterwards.
#[cfg(feature = "peios-boundary")]
fn create_or_open_directory(
    path: &Path,
    sd: &ResolvedSecurity,
) -> io::Result<(peios::file::File, bool)> {
    let ResolvedSecurity::Descriptor(sd) = sd;
    match OpenOptions::new()
        .desired_access(DIRECTORY_CREATE_ACCESS)
        .disposition(Disposition::Create)
        .options(CreateOptions::DIRECTORY)
        .flags(OpenFlags::SYMLINK_NOFOLLOW)
        .creator_sd(sd)
        .create(None, path)
    {
        Ok((file, _)) => return Ok((file, true)),
        Err(error) if error.raw_os_error() == Some(libc::EEXIST) => {}
        Err(error) => return Err(io::Error::from(error)),
    }
    let (file, _) = OpenOptions::new()
        .desired_access(DIRECTORY_CREATE_ACCESS | FileAccess::WRITE_DAC | FileAccess::WRITE_OWNER)
        .disposition(Disposition::Open)
        .options(CreateOptions::DIRECTORY)
        .flags(OpenFlags::SYMLINK_NOFOLLOW)
        .create(None, path)
        .map_err(io::Error::from)?;
    Ok((file, false))
}

#[cfg(not(feature = "peios-boundary"))]
fn create_or_open_directory(
    path: &Path,
    _sd: &ResolvedSecurity,
) -> io::Result<(std::fs::File, bool)> {
    let created = match std::fs::create_dir(path) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => false,
        Err(error) => return Err(error),
    };
    if !path.symlink_metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} exists but is not a directory", path.display()),
        ));
    }
    std::fs::File::open(path).map(|file| (file, created))
}

/// The file counterpart of [`create_or_open_directory`], split for the same
/// reason.
#[cfg(feature = "peios-boundary")]
fn create_or_open_file(
    path: &Path,
    sd: &ResolvedSecurity,
) -> io::Result<(peios::file::File, bool)> {
    let ResolvedSecurity::Descriptor(sd) = sd;
    match OpenOptions::new()
        .desired_access(FILE_CREATE_ACCESS)
        .disposition(Disposition::Create)
        .flags(OpenFlags::SYMLINK_NOFOLLOW)
        .creator_sd(sd)
        .create(None, path)
    {
        Ok((file, _)) => return Ok((file, true)),
        Err(error) if error.raw_os_error() == Some(libc::EEXIST) => {}
        Err(error) => return Err(io::Error::from(error)),
    }
    let (file, _) = OpenOptions::new()
        .desired_access(FILE_CREATE_ACCESS | FileAccess::WRITE_DAC | FileAccess::WRITE_OWNER)
        .disposition(Disposition::Open)
        .flags(OpenFlags::SYMLINK_NOFOLLOW)
        .create(None, path)
        .map_err(io::Error::from)?;
    Ok((file, false))
}

#[cfg(not(feature = "peios-boundary"))]
fn create_or_open_file(path: &Path, _sd: &ResolvedSecurity) -> io::Result<(std::fs::File, bool)> {
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => return Ok((file, true)),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map(|file| (file, false))
}

fn fd_is_regular_file(fd: i32) -> io::Result<bool> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let stat = unsafe { stat.assume_init() };
    Ok((stat.st_mode & libc::S_IFMT) == libc::S_IFREG)
}

#[cfg(feature = "peios-boundary")]
fn apply_fd_security(file: &peios::file::File, sd: &ResolvedSecurity) -> io::Result<()> {
    let ResolvedSecurity::Descriptor(sd) = sd;
    file.fd_set_sd(SecInfo::OWNER | SecInfo::GROUP | SecInfo::DACL, sd)
        .map_err(io::Error::from)
}

#[cfg(not(feature = "peios-boundary"))]
fn apply_fd_security(_file: &std::fs::File, _sd: &ResolvedSecurity) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provisioning::{ProvisionedPath, ProvisionedPathKind, ProvisionedPathSecurity};

    #[test]
    fn optional_boot_path_failure_is_reported_as_warning() {
        let report = provision_linux_boot_paths(&[ProvisionedPath {
            name: "missing-parent".to_string(),
            kind: ProvisionedPathKind::Directory,
            path: "/tmp/peinit-test-missing-parent/child".to_string(),
            security: ProvisionedPathSecurity::Default,
            required: false,
        }]);

        assert!(report.applied.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(report.required_failures.is_empty());
    }

    /// A provisioning failure must name the step and the path. Both the runtime
    /// directory and the boot path routes reach the operator as one line on the
    /// console, and "Invalid argument" with three candidate syscalls behind it
    /// costs a boot to narrow.
    #[test]
    fn provisioning_failure_names_the_step_and_the_path() {
        let report = provision_linux_boot_paths(&[ProvisionedPath {
            name: "missing-parent".to_string(),
            kind: ProvisionedPathKind::Directory,
            path: "/tmp/peinit-test-missing-parent/child".to_string(),
            security: ProvisionedPathSecurity::Default,
            required: false,
        }]);

        let message = &report.warnings[0].message;
        assert!(
            message.contains("/tmp/peinit-test-missing-parent"),
            "message should name the path: {message}",
        );
    }

    /// The create-then-open split has two branches and provisioning is meant to
    /// be idempotent, so both have to work on the same path in a row.
    ///
    /// This exercises the non-KACS variant -- the real bug was in the KACS
    /// masks, which no host test can reach -- but it does pin the `created`
    /// flag, which is what decides whether the descriptor is applied a second
    /// time.
    #[test]
    fn provisioning_a_path_twice_creates_it_once_and_then_opens_it() {
        let root =
            std::path::PathBuf::from(format!("/tmp/peinit-test-provision-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("test root");
        let directory = root.join("dir");
        let file = root.join("file");

        let paths = vec![
            ProvisionedPath {
                name: "dir".to_string(),
                kind: ProvisionedPathKind::Directory,
                path: directory.display().to_string(),
                security: ProvisionedPathSecurity::Default,
                required: true,
            },
            ProvisionedPath {
                name: "file".to_string(),
                kind: ProvisionedPathKind::File,
                path: file.display().to_string(),
                security: ProvisionedPathSecurity::Default,
                required: true,
            },
        ];

        for pass in 0..2 {
            let report = provision_linux_boot_paths(&paths);
            assert!(
                report.required_failures.is_empty() && report.warnings.is_empty(),
                "pass {pass} failed: {report:?}",
            );
            assert_eq!(report.applied, vec!["dir".to_string(), "file".to_string()]);
        }
        assert!(directory.is_dir());
        assert!(file.is_file());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn required_boot_path_failure_is_reported_as_required_failure() {
        let report = provision_linux_boot_paths(&[ProvisionedPath {
            name: "missing-parent".to_string(),
            kind: ProvisionedPathKind::File,
            path: "/tmp/peinit-test-missing-parent/file".to_string(),
            security: ProvisionedPathSecurity::Default,
            required: true,
        }]);

        assert!(report.applied.is_empty());
        assert!(report.warnings.is_empty());
        assert_eq!(report.required_failures.len(), 1);
    }
}
