use super::model::NotifyApplyError;

pub(super) fn unknown_service(service: &str) -> NotifyApplyError {
    NotifyApplyError::ServiceTable(crate::service::ServiceTableError::UnknownService {
        service: service.to_string(),
    })
}
