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

#[test]
fn growing_capacity_keeps_every_record() {
    let mut buffer = PreEventdLogBuffer::new(96);
    buffer.push(record("one"));
    buffer.push(record("two"));
    buffer.push(record("three"));
    let used = buffer.used_bytes();

    buffer.set_capacity_bytes(4096);

    assert_eq!(buffer.capacity_bytes(), 4096);
    assert_eq!(buffer.used_bytes(), used);
    assert_eq!(
        buffer
            .records()
            .iter()
            .map(|record| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["two", "three"],
    );
}

/// A grown buffer must actually hold more, not merely report a larger number.
#[test]
fn growing_capacity_admits_records_the_old_capacity_would_have_evicted() {
    let mut buffer = PreEventdLogBuffer::new(96);
    buffer.push(record("one"));
    buffer.push(record("two"));

    buffer.set_capacity_bytes(4096);
    buffer.push(record("three"));
    buffer.push(record("four"));

    assert_eq!(
        buffer
            .records()
            .iter()
            .map(|record| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "two", "three", "four"],
    );
}

/// Shrinking drops from the front, the same end `push` drops on overflow.
#[test]
fn shrinking_capacity_evicts_oldest_until_contents_fit() {
    let mut buffer = PreEventdLogBuffer::new(4096);
    buffer.push(record("one"));
    buffer.push(record("two"));
    buffer.push(record("three"));

    buffer.set_capacity_bytes(96);

    assert_eq!(buffer.capacity_bytes(), 96);
    assert!(buffer.used_bytes() <= 96);
    // The same two survivors a steady-state overrun leaves at this capacity —
    // see `drops_oldest_records_when_capacity_is_exceeded`.
    assert_eq!(
        buffer
            .records()
            .iter()
            .map(|record| record.message.as_str())
            .collect::<Vec<_>>(),
        vec!["two", "three"],
    );
}

/// The degenerate shrink: no capacity can hold anything, so the buffer empties
/// rather than being left reporting used bytes it no longer has room for.
#[test]
fn shrinking_to_zero_empties_the_buffer() {
    let mut buffer = PreEventdLogBuffer::new(4096);
    buffer.push(record("one"));
    buffer.push(record("two"));

    buffer.set_capacity_bytes(0);

    assert!(buffer.is_empty());
    assert_eq!(buffer.used_bytes(), 0);
}

fn record(message: &str) -> ServiceLogRecord {
    ServiceLogRecord::new("app", LogStream::Stdout, message, 1, None)
}
