use std::os::fd::AsRawFd;

use crate::boundary::{
    BoundaryError, RegistryWatchEvent, RegistryWatchEventKind, RegistryWatchRoot,
    RegistryWatchSource,
};

use super::{INIT_ROOT_KEY, SERVICES_ROOT_KEY};

const WATCH_READ_BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub struct LcsRegistryWatch {
    root: RegistryWatchRoot,
    key: peios::registry::Key,
}

impl LcsRegistryWatch {
    pub fn open_armed(root: RegistryWatchRoot) -> Result<Self, BoundaryError> {
        use peios::registry::{Key, KeyAccess, NotifyFilter, OpenFlags};

        let key = Key::open(
            None,
            root_path(root),
            KeyAccess::NOTIFY,
            OpenFlags::default(),
        )
        .map_err(|error| watch_error(root, format!("open failed: {error:?}")))?;
        key.set_nonblocking(true)
            .map_err(|error| watch_error(root, format!("set nonblocking failed: {error}")))?;
        key.notify(NotifyFilter::ALL, true)
            .map_err(|error| watch_error(root, format!("notify arm failed: {error:?}")))?;
        Ok(Self { root, key })
    }

    pub fn fd(&self) -> i32 {
        self.key.as_raw_fd()
    }

    pub fn root(&self) -> RegistryWatchRoot {
        self.root
    }

    fn read_events(&self) -> Result<Vec<RegistryWatchEvent>, BoundaryError> {
        let mut buffer = vec![0u8; WATCH_READ_BUFFER_BYTES];
        self.key
            .read_watch_events(&mut buffer)
            .map_err(|error| watch_error(self.root, format!("read failed: {error:?}")))?
            .into_iter()
            .map(|event| registry_watch_event_from_peios(self.root, event))
            .collect()
    }
}

#[derive(Debug, Default)]
pub struct LcsRegistryWatches {
    watches: Vec<LcsRegistryWatch>,
}

impl LcsRegistryWatches {
    pub fn push(&mut self, watch: LcsRegistryWatch) {
        self.watches.push(watch);
    }

    pub fn is_empty(&self) -> bool {
        self.watches.is_empty()
    }
}

impl RegistryWatchSource for LcsRegistryWatches {
    fn drain_registry_watch_events(
        &mut self,
        fd: i32,
    ) -> Result<Vec<RegistryWatchEvent>, BoundaryError> {
        let watch = self
            .watches
            .iter()
            .find(|watch| watch.fd() == fd)
            .ok_or_else(|| BoundaryError::Registry(format!("unknown registry watch fd {fd}")))?;
        watch.read_events()
    }
}

fn root_path(root: RegistryWatchRoot) -> &'static str {
    match root {
        RegistryWatchRoot::Services => SERVICES_ROOT_KEY,
        RegistryWatchRoot::Init => INIT_ROOT_KEY,
    }
}

fn watch_error(root: RegistryWatchRoot, message: String) -> BoundaryError {
    BoundaryError::Registry(format!("registry watch {root:?}: {message}"))
}

fn registry_watch_event_from_peios(
    root: RegistryWatchRoot,
    event: peios::registry::WatchEvent,
) -> Result<RegistryWatchEvent, BoundaryError> {
    Ok(RegistryWatchEvent {
        root,
        kind: registry_watch_kind_from_peios(event.event_type),
        name: decode_watch_component(root, event.name)?,
        path: event
            .path
            .into_iter()
            .map(|component| decode_watch_component(root, component))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn registry_watch_kind_from_peios(kind: peios::registry::WatchEventType) -> RegistryWatchEventKind {
    match kind {
        peios::registry::WatchEventType::ValueSet => RegistryWatchEventKind::ValueSet,
        peios::registry::WatchEventType::ValueDeleted => RegistryWatchEventKind::ValueDeleted,
        peios::registry::WatchEventType::SubkeyCreated => RegistryWatchEventKind::SubkeyCreated,
        peios::registry::WatchEventType::SubkeyDeleted => RegistryWatchEventKind::SubkeyDeleted,
        peios::registry::WatchEventType::SecurityDescriptorChanged => {
            RegistryWatchEventKind::SecurityDescriptorChanged
        }
        peios::registry::WatchEventType::KeyDeleted => RegistryWatchEventKind::KeyDeleted,
        peios::registry::WatchEventType::Overflow => RegistryWatchEventKind::Overflow,
        peios::registry::WatchEventType::Other(raw) => RegistryWatchEventKind::Other(raw),
    }
}

fn decode_watch_component(
    root: RegistryWatchRoot,
    component: Vec<u8>,
) -> Result<String, BoundaryError> {
    String::from_utf8(component)
        .map_err(|error| watch_error(root, format!("invalid UTF-8 watch component: {error}")))
}
