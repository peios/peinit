use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};

use peios::file::{CreateOptions, Disposition, FileAccess, OpenOptions};

use crate::boundary::BoundaryError;

pub(super) fn create_cgroup(cgroup_id: &str) -> Result<(), BoundaryError> {
    if cgroup_id.is_empty() {
        return Err(BoundaryError::Process("empty cgroup id".to_string()));
    }
    let mut current = PathBuf::new();
    for component in Path::new(cgroup_id).components() {
        current.push(component.as_os_str());
        if current == Path::new("/") {
            continue;
        }
        OpenOptions::new()
            .desired_access(FileAccess::LIST_DIRECTORY | FileAccess::TRAVERSE)
            .disposition(Disposition::OpenIf)
            .options(CreateOptions::DIRECTORY)
            .open(None, &current)
            .map(drop)
            .map_err(|error| {
                BoundaryError::Process(format!(
                    "create filesystem check cgroup {} failed: {error}",
                    current.display(),
                ))
            })?;
    }
    Ok(())
}

pub(super) fn open_cgroup_directory(cgroup_id: &str) -> Result<OwnedFd, BoundaryError> {
    let file = OpenOptions::new()
        .desired_access(FileAccess::LIST_DIRECTORY | FileAccess::TRAVERSE)
        .options(CreateOptions::DIRECTORY)
        .open(None, Path::new(cgroup_id))
        .map_err(|error| {
            BoundaryError::Process(format!("open cgroup directory {cgroup_id} failed: {error}"))
        })?;
    Ok(file.into())
}
