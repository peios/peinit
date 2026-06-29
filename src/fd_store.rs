use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::Arc;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FdStoreTable {
    stores: BTreeMap<String, FdStore>,
}

impl FdStoreTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn store(
        &mut self,
        service: &str,
        max_entries: u32,
        request: StoreFdRequest,
    ) -> StoreFdOutcome {
        if max_entries == 0 {
            return StoreFdOutcome::Disabled;
        }
        self.stores
            .entry(service.to_string())
            .or_default()
            .store(max_entries, request)
    }

    pub fn remove_name(&mut self, service: &str, name: &str) -> usize {
        let Some(store) = self.stores.get_mut(service) else {
            return 0;
        };
        let removed = store.remove_name(name);
        if store.is_empty() {
            self.stores.remove(service);
        }
        removed
    }

    pub fn clear_service(&mut self, service: &str) {
        self.stores.remove(service);
    }

    pub fn retain_services(&mut self, services: &[&str]) -> Vec<String> {
        let retained = services.iter().copied().collect::<BTreeSet<_>>();
        let removed = self
            .stores
            .keys()
            .filter(|service| !retained.contains(service.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        for service in &removed {
            self.stores.remove(service);
        }
        removed
    }

    pub fn service(&self, service: &str) -> Option<&FdStore> {
        self.stores.get(service)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FdStore {
    entries: Vec<StoredFd>,
}

impl FdStore {
    pub const DEFAULT_NAME: &'static str = "stored";

    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn store(&mut self, max_entries: u32, request: StoreFdRequest) -> StoreFdOutcome {
        if max_entries == 0 {
            return StoreFdOutcome::Disabled;
        }
        let max_entries = usize::try_from(max_entries).unwrap_or(usize::MAX);
        if self.entries.len() >= max_entries {
            return StoreFdOutcome::Full;
        }
        self.entries.push(StoredFd {
            name: stored_name(request.name),
            poll: request.poll,
            fd: SharedFd::new(request.fd),
        });
        StoreFdOutcome::Stored
    }

    pub fn remove_name(&mut self, name: &str) -> usize {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.name != name);
        before - self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn entries(&self) -> &[StoredFd] {
        &self.entries
    }
}

#[derive(Debug)]
pub struct StoreFdRequest {
    pub name: Option<String>,
    pub poll: bool,
    pub fd: OwnedFd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreFdOutcome {
    Stored,
    Disabled,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFd {
    pub name: String,
    pub poll: bool,
    pub fd: SharedFd,
}

#[derive(Debug, Clone)]
pub struct SharedFd {
    inner: Arc<OwnedFd>,
}

impl SharedFd {
    fn new(fd: OwnedFd) -> Self {
        Self {
            inner: Arc::new(fd),
        }
    }

    pub fn raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }

    pub fn duplicate(&self) -> std::io::Result<OwnedFd> {
        self.inner.as_fd().try_clone_to_owned()
    }

    pub fn duplicate_min(&self, min_fd: RawFd) -> std::io::Result<OwnedFd> {
        let fd = unsafe { libc::fcntl(self.raw_fd(), libc::F_DUPFD_CLOEXEC, min_fd) };
        if fd < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(unsafe { OwnedFd::from_raw_fd(fd) })
        }
    }
}

impl PartialEq for SharedFd {
    fn eq(&self, other: &Self) -> bool {
        self.raw_fd() == other.raw_fd()
    }
}

impl Eq for SharedFd {}

fn stored_name(name: Option<String>) -> String {
    name.filter(|name| !name.is_empty())
        .unwrap_or_else(|| FdStore::DEFAULT_NAME.to_string())
}

#[cfg(test)]
mod tests;
