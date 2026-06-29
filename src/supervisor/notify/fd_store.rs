use std::os::fd::OwnedFd;

use crate::execution::notify::{NotifyApplyDispatch, NotifyApplyError};
use crate::fd_store::{FdStore, StoreFdOutcome, StoreFdRequest};
use crate::notify::{NotifyField, NotifyMessage};
use crate::service::ServiceTableError;

use crate::supervisor::dispatch::SupervisorFdStoreRejectionDispatch;
use crate::supervisor::work::SupervisorWork;

pub(super) fn apply_fd_store_notify_fields(
    work: &mut SupervisorWork,
    message: &NotifyMessage,
    notify: &NotifyApplyDispatch,
    fds: Vec<OwnedFd>,
) -> Result<Vec<SupervisorFdStoreRejectionDispatch>, NotifyApplyError> {
    let directive = FdStoreDirective::from_message(message);
    if directive.remove {
        let Some(name) = directive.name.as_deref() else {
            return Ok(Vec::new());
        };
        work.fd_store.remove_name(&notify.sender.service, name);
    }
    if !directive.store {
        return Ok(Vec::new());
    }

    let max_entries = work
        .services
        .definition(&notify.sender.service)
        .ok_or_else(|| {
            NotifyApplyError::ServiceTable(ServiceTableError::UnknownService {
                service: notify.sender.service.clone(),
            })
        })?
        .fd_store_max;
    let mut rejections = Vec::new();
    for fd in fds {
        let name = directive.effective_name();
        let outcome = work.fd_store.store(
            &notify.sender.service,
            max_entries,
            StoreFdRequest {
                name: Some(name.clone()),
                poll: directive.poll,
                fd,
            },
        );
        match outcome {
            StoreFdOutcome::Stored => {}
            StoreFdOutcome::Disabled | StoreFdOutcome::Full => {
                rejections.push(SupervisorFdStoreRejectionDispatch {
                    service: notify.sender.service.clone(),
                    name,
                    outcome,
                });
            }
        }
    }
    Ok(rejections)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FdStoreDirective {
    store: bool,
    remove: bool,
    name: Option<String>,
    poll: bool,
}

impl FdStoreDirective {
    fn from_message(message: &NotifyMessage) -> Self {
        let mut directive = Self {
            store: false,
            remove: false,
            name: None,
            poll: true,
        };
        for field in &message.fields {
            match field {
                NotifyField::FdStore => directive.store = true,
                NotifyField::FdStoreRemove => directive.remove = true,
                NotifyField::FdName(name) => {
                    directive.name = Some(name.clone());
                }
                NotifyField::FdPoll(value) if value == "0" => directive.poll = false,
                _ => {}
            }
        }
        if directive.store && directive.name.is_none() {
            directive.name = Some(FdStore::DEFAULT_NAME.to_string());
        }
        directive
    }

    fn effective_name(&self) -> String {
        self.name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or(FdStore::DEFAULT_NAME)
            .to_string()
    }
}
