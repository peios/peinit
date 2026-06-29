use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;

use super::{FdStore, FdStoreTable, StoreFdOutcome, StoreFdRequest};

#[test]
fn stores_fds_in_arrival_order_with_default_name() {
    let mut store = FdStore::new();
    assert_eq!(store.store(2, request(None)), StoreFdOutcome::Stored);
    assert_eq!(
        store.store(2, request(Some("listener"))),
        StoreFdOutcome::Stored
    );

    assert_eq!(store.len(), 2);
    assert_eq!(store.entries()[0].name, "stored");
    assert_eq!(store.entries()[1].name, "listener");
}

#[test]
fn disabled_or_full_store_drops_new_fd_without_modifying_existing_entries() {
    let mut store = FdStore::new();
    assert_eq!(
        store.store(0, request(Some("ignored"))),
        StoreFdOutcome::Disabled
    );
    assert!(store.is_empty());

    assert_eq!(
        store.store(1, request(Some("first"))),
        StoreFdOutcome::Stored
    );
    let raw = store.entries()[0].fd.raw_fd();
    assert_eq!(
        store.store(1, request(Some("second"))),
        StoreFdOutcome::Full
    );

    assert_eq!(store.len(), 1);
    assert_eq!(store.entries()[0].name, "first");
    assert_eq!(store.entries()[0].fd.raw_fd(), raw);
}

#[test]
fn removes_all_fds_with_matching_name() {
    let mut store = FdStore::new();
    store.store(4, request(Some("listener")));
    store.store(4, request(Some("cache")));
    store.store(4, request(Some("listener")));

    assert_eq!(store.remove_name("listener"), 2);
    assert_eq!(store.entries()[0].name, "cache");
    assert_eq!(store.remove_name("missing"), 0);
}

#[test]
fn cloned_store_shares_fd_ownership_without_duplication() {
    let mut store = FdStore::new();
    store.store(1, request(Some("listener")));
    let cloned = store.clone();

    assert_eq!(store.entries()[0].fd, cloned.entries()[0].fd);
    assert!(cloned.entries()[0].fd.duplicate().is_ok());
}

#[test]
fn table_isolates_entries_by_service() {
    let mut table = FdStoreTable::new();
    assert_eq!(
        table.store("app", 2, request(Some("listener"))),
        StoreFdOutcome::Stored,
    );
    assert_eq!(
        table.store("db", 2, request(Some("listener"))),
        StoreFdOutcome::Stored,
    );

    assert_eq!(table.remove_name("app", "listener"), 1);
    assert!(table.service("app").is_none());
    assert_eq!(table.service("db").expect("db store").len(), 1);
}

#[test]
fn table_drops_stores_for_services_not_retained() {
    let mut table = FdStoreTable::new();
    table.store("app", 2, request(Some("listener")));
    table.store("db", 2, request(Some("listener")));

    assert_eq!(table.retain_services(&["db"]), vec!["app"]);

    assert!(table.service("app").is_none());
    assert_eq!(table.service("db").expect("db store").len(), 1);
}

fn request(name: Option<&str>) -> StoreFdRequest {
    StoreFdRequest {
        name: name.map(ToString::to_string),
        poll: true,
        fd: test_fd(),
    }
}

fn test_fd() -> OwnedFd {
    let (left, _right) = UnixStream::pair().expect("socket pair");
    left.into()
}
