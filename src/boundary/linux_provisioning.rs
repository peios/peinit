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
        let sd = service_runtime_directory_security(service)?;
        ensure_directory(&path, &sd)?;
    }
    Ok(())
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
    let directory = create_or_open_directory(path, sd)?;
    apply_fd_security(&directory, sd)
}

fn ensure_file(path: &Path, sd: &ResolvedSecurity) -> io::Result<()> {
    ensure_parent_exists(path)?;
    let file = create_or_open_file(path, sd)?;
    if !fd_is_regular_file(file.as_raw_fd())? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} exists but is not a regular file", path.display()),
        ));
    }
    apply_fd_security(&file, sd)
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
    if parent.symlink_metadata()?.is_dir() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("parent is not a directory: {}", parent.display()),
        ))
    }
}

#[cfg(feature = "peios-boundary")]
fn create_or_open_directory(path: &Path, sd: &ResolvedSecurity) -> io::Result<peios::file::File> {
    let ResolvedSecurity::Descriptor(sd) = sd;
    let (file, _) = OpenOptions::new()
        .desired_access(
            FileAccess::READ_ATTRIBUTES
                | FileAccess::WRITE_DAC
                | FileAccess::WRITE_OWNER
                | FileAccess::SYNCHRONIZE,
        )
        .disposition(Disposition::OpenIf)
        .options(CreateOptions::DIRECTORY)
        .flags(OpenFlags::SYMLINK_NOFOLLOW)
        .creator_sd(sd)
        .create(None, path)
        .map_err(io::Error::from)?;
    Ok(file)
}

#[cfg(not(feature = "peios-boundary"))]
fn create_or_open_directory(path: &Path, _sd: &ResolvedSecurity) -> io::Result<std::fs::File> {
    match std::fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    if !path.symlink_metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} exists but is not a directory", path.display()),
        ));
    }
    std::fs::File::open(path)
}

#[cfg(feature = "peios-boundary")]
fn create_or_open_file(path: &Path, sd: &ResolvedSecurity) -> io::Result<peios::file::File> {
    let ResolvedSecurity::Descriptor(sd) = sd;
    let (file, _) = OpenOptions::new()
        .desired_access(
            FileAccess::READ_DATA
                | FileAccess::WRITE_DATA
                | FileAccess::READ_ATTRIBUTES
                | FileAccess::WRITE_DAC
                | FileAccess::WRITE_OWNER
                | FileAccess::SYNCHRONIZE,
        )
        .disposition(Disposition::OpenIf)
        .flags(OpenFlags::SYMLINK_NOFOLLOW)
        .creator_sd(sd)
        .create(None, path)
        .map_err(io::Error::from)?;
    Ok(file)
}

#[cfg(not(feature = "peios-boundary"))]
fn create_or_open_file(path: &Path, _sd: &ResolvedSecurity) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
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
