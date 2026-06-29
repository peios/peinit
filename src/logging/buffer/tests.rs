use crate::logging::{LogStream, ServiceLogRecord};

use super::PreEventdLogBuffer;

#[test]
fn drops_oldest_records_when_capacity_is_exceeded() {
    let mut buffer = PreEventdLogBuffer::new(96);

    buffer.push(record("one"));
    buffer.push(record("two"));
    buffer.push(record("three"));

    assert_eq!(
        buffer
            .records()
            .iter()
            .map(|record| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["two", "three"],
    );
}

#[test]
fn oversized_record_is_dropped() {
    let mut buffer = PreEventdLogBuffer::new(8);

    buffer.push(record("too large"));

    assert!(buffer.is_empty());
}

#[test]
fn popping_front_updates_used_bytes() {
    let mut buffer = PreEventdLogBuffer::new(256);
    buffer.push(record("one"));
    buffer.push(record("two"));
    let used = buffer.used_bytes();

    assert_eq!(buffer.pop_front().expect("record").message.as_str(), "one",);
    assert!(buffer.used_bytes() < used);
    assert_eq!(buffer.front().expect("record").message, "two");
}

fn record(message: &str) -> ServiceLogRecord {
    ServiceLogRecord::new("app", LogStream::Stdout, message, 1, None)
}
