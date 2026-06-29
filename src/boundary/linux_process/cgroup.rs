use std::ffi::CString;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use peios::file::{FileAccess, OpenOptions};

use crate::boundary::{BoundaryError, CgroupRemoveOutcome, read_fd_to_string, write_all_fd};

pub(super) fn kill_cgroup(cgroup_id: &str) -> Result<(), BoundaryError> {
    let path = cgroup_kill_file_path(cgroup_id)?;
    let file = OpenOptions::new()
        .desired_access(FileAccess::WRITE_DATA)
        .open(None, &path)
        .map_err(|error| {
            BoundaryError::Process(format!("open {} failed: {error}", path.display()))
        })?;
    write_all_fd(file.as_raw_fd(), b"1").map_err(|error| {
        BoundaryError::Process(format!("write {} failed: {error}", path.display()))
    })
}

pub(super) fn cgroup_populated(cgroup_id: &str) -> Result<bool, BoundaryError> {
    let path = cgroup_events_file_path(cgroup_id)?;
    let file = OpenOptions::new()
        .desired_access(FileAccess::READ_DATA)
        .open(None, &path)
        .map_err(|error| {
            BoundaryError::Process(format!("open {} failed: {error}", path.display()))
        })?;
    let text = read_fd_to_string(file.as_raw_fd()).map_err(|error| {
        BoundaryError::Process(format!("read {} failed: {error}", path.display()))
    })?;
    parse_cgroup_populated(&text).ok_or_else(|| {
        BoundaryError::Process(format!("{} missing populated field", path.display()))
    })
}

pub(super) fn remove_cgroup_directory(
    cgroup_id: &str,
) -> Result<CgroupRemoveOutcome, BoundaryError> {
    if cgroup_id.is_empty() {
        return Err(BoundaryError::Process("empty cgroup id".to_string()));
    }
    let path = Path::new(cgroup_id);
    let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        BoundaryError::Process(format!("cgroup path contains NUL byte: {}", path.display()))
    })?;
    if unsafe { libc::rmdir(c_path.as_ptr()) } == 0 {
        return Ok(CgroupRemoveOutcome::Removed);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ENOENT) => Ok(CgroupRemoveOutcome::Missing),
        Some(libc::EBUSY | libc::ENOTEMPTY) => Ok(CgroupRemoveOutcome::Busy),
        _ => Err(BoundaryError::Process(format!(
            "rmdir {} failed: {error}",
            path.display()
        ))),
    }
}

fn cgroup_kill_file_path(cgroup_id: &str) -> Result<PathBuf, BoundaryError> {
    if cgroup_id.is_empty() {
        return Err(BoundaryError::Process("empty cgroup id".to_string()));
    }
    Ok(Path::new(cgroup_id).join("cgroup.kill"))
}

fn cgroup_events_file_path(cgroup_id: &str) -> Result<PathBuf, BoundaryError> {
    if cgroup_id.is_empty() {
        return Err(BoundaryError::Process("empty cgroup id".to_string()));
    }
    Ok(Path::new(cgroup_id).join("cgroup.events"))
}

fn parse_cgroup_populated(text: &str) -> Option<bool> {
    text.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next()? == "populated").then(|| match fields.next()? {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        })?
    })
}

#[cfg(test)]
mod tests {
    use super::{cgroup_events_file_path, cgroup_kill_file_path, parse_cgroup_populated};

    #[test]
    fn builds_cgroup_kill_file_path() {
        assert_eq!(
            cgroup_kill_file_path("/sys/fs/cgroup/peinit/app.gen1")
                .expect("cgroup path")
                .to_string_lossy(),
            "/sys/fs/cgroup/peinit/app.gen1/cgroup.kill",
        );
        assert!(cgroup_kill_file_path("").is_err());
    }

    #[test]
    fn builds_cgroup_events_file_path() {
        assert_eq!(
            cgroup_events_file_path("/sys/fs/cgroup/peinit/app.gen1/main")
                .expect("cgroup path")
                .to_string_lossy(),
            "/sys/fs/cgroup/peinit/app.gen1/main/cgroup.events",
        );
        assert!(cgroup_events_file_path("").is_err());
    }

    #[test]
    fn parses_cgroup_populated_field() {
        assert_eq!(
            parse_cgroup_populated("populated 0\nfrozen 0\n"),
            Some(false)
        );
        assert_eq!(
            parse_cgroup_populated("frozen 0\npopulated 1\n"),
            Some(true)
        );
        assert_eq!(parse_cgroup_populated("populated maybe\n"), None);
        assert_eq!(parse_cgroup_populated("frozen 0\n"), None);
    }
}
