#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyMessage {
    pub fields: Vec<NotifyField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyField {
    Ready,
    Reloading,
    Stopping,
    Status(String),
    Errno(String),
    ExitStatus(String),
    Watchdog,
    WatchdogUsec(String),
    ExtendTimeoutUsec(String),
    FdStore,
    FdName(String),
    FdStoreRemove,
    FdPoll(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotifyParseError {
    InvalidUtf8,
    MalformedLine { line_index: usize },
}

pub fn parse_notify_message(payload: &[u8]) -> Result<NotifyMessage, NotifyParseError> {
    let text = std::str::from_utf8(payload).map_err(|_| NotifyParseError::InvalidUtf8)?;
    let mut fields = Vec::new();

    for (line_index, line) in text.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(NotifyParseError::MalformedLine { line_index });
        };
        if key.is_empty() {
            return Err(NotifyParseError::MalformedLine { line_index });
        }
        if let Some(field) = supported_field(key, value) {
            fields.push(field);
        }
    }

    Ok(NotifyMessage { fields })
}

fn supported_field(key: &str, value: &str) -> Option<NotifyField> {
    match key {
        "READY" if value == "1" => Some(NotifyField::Ready),
        "RELOADING" if value == "1" => Some(NotifyField::Reloading),
        "STOPPING" if value == "1" => Some(NotifyField::Stopping),
        "STATUS" => Some(NotifyField::Status(value.to_string())),
        "ERRNO" => Some(NotifyField::Errno(value.to_string())),
        "EXIT_STATUS" => Some(NotifyField::ExitStatus(value.to_string())),
        "WATCHDOG" if value == "1" => Some(NotifyField::Watchdog),
        "WATCHDOG_USEC" => Some(NotifyField::WatchdogUsec(value.to_string())),
        "EXTEND_TIMEOUT_USEC" => Some(NotifyField::ExtendTimeoutUsec(value.to_string())),
        "FDSTORE" if value == "1" => Some(NotifyField::FdStore),
        "FDNAME" => Some(NotifyField::FdName(value.to_string())),
        "FDSTOREREMOVE" if value == "1" => Some(NotifyField::FdStoreRemove),
        "FDPOLL" => Some(NotifyField::FdPoll(value.to_string())),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
