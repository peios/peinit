use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use peios::file::{CreateOptions, Disposition, FileAccess, OpenOptions};

use crate::boundary::BoundaryError;
use crate::job::{JobRecord, JobType};

pub(super) fn create_job_cgroups(job: &JobRecord) -> Result<(), BoundaryError> {
    match job.job_type {
        JobType::ServiceMain => create_service_main_cgroup_tree(&job.cgroup_id),
        JobType::PreExecHook | JobType::PostExecHook | JobType::ReloadHook => {
            create_cgroup(&job.cgroup_id)
        }
        JobType::HealthCheck | JobType::AdHoc => create_cgroup(&job.cgroup_id),
    }
}

pub(super) fn open_cgroup_directory(cgroup_id: &str) -> Result<OwnedFd, BoundaryError> {
    let file = OpenOptions::new()
        .desired_access(FileAccess::LIST_DIRECTORY | FileAccess::TRAVERSE)
        .options(CreateOptions::DIRECTORY)
        .open(None, Path::new(cgroup_id))
        .map_err(|error| {
            BoundaryError::Process(format!("open cgroup directory {cgroup_id} failed: {error}"))
        })?;
    let fd: OwnedFd = file.into();
    // child_exec closes the stdio pipes, /dev/null, the console and the token
    // fd by hand, but not this one — so without CLOEXEC every service inherits
    // a traversable descriptor on its own cgroup directory.
    crate::boundary::set_cloexec(fd.as_raw_fd()).map_err(|error| {
        BoundaryError::Process(format!(
            "cloexec cgroup directory {cgroup_id} failed: {error}"
        ))
    })?;
    Ok(fd)
}

fn create_service_main_cgroup_tree(main_cgroup_id: &str) -> Result<(), BoundaryError> {
    let main = Path::new(main_cgroup_id);
    let root = main
        .parent()
        .ok_or_else(|| BoundaryError::Process(format!("invalid main cgroup {main_cgroup_id}")))?;
    create_cgroup_path(root)?;
    for child in ["main", "hooks", "health"] {
        create_cgroup_path(&root.join(child))?;
    }
    Ok(())
}

fn create_cgroup(cgroup_id: &str) -> Result<(), BoundaryError> {
    if cgroup_id.is_empty() {
        return Err(BoundaryError::Process("empty cgroup id".to_string()));
    }
    create_cgroup_path(Path::new(cgroup_id))
}

fn create_cgroup_path(path: &Path) -> Result<(), BoundaryError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if current == Path::new("/") {
            continue;
        }
        create_cgroup_directory(&current)?;
    }
    Ok(())
}

fn create_cgroup_directory(path: &Path) -> Result<(), BoundaryError> {
    OpenOptions::new()
        .desired_access(FileAccess::LIST_DIRECTORY | FileAccess::TRAVERSE)
        .disposition(Disposition::OpenIf)
        .options(CreateOptions::DIRECTORY)
        .open(None, path)
        .map(drop)
        .map_err(|error| {
            BoundaryError::Process(format!(
                "create cgroup directory {} failed: {error}",
                path.display(),
            ))
        })
}

#[cfg(test)]
fn cgroup_procs_file_path(cgroup_id: &str) -> Result<PathBuf, BoundaryError> {
    if cgroup_id.is_empty() {
        return Err(BoundaryError::Process("empty cgroup id".to_string()));
    }
    Ok(Path::new(cgroup_id).join("cgroup.procs"))
}

#[cfg(test)]
mod tests {
    use super::cgroup_procs_file_path;

    #[test]
    fn cgroup_procs_path_targets_cgroup_procs_file() {
        assert_eq!(
            cgroup_procs_file_path("/sys/fs/cgroup/peinit/app.gen1/main")
                .expect("path")
                .to_string_lossy(),
            "/sys/fs/cgroup/peinit/app.gen1/main/cgroup.procs",
        );
        assert!(cgroup_procs_file_path("").is_err());
    }

    #[test]
    fn service_main_tree_derives_root_from_main_cgroup() {
        let root = std::path::Path::new("/sys/fs/cgroup/peinit/app.gen1/main")
            .parent()
            .expect("root");
        assert_eq!(root.to_string_lossy(), "/sys/fs/cgroup/peinit/app.gen1");
    }
}
