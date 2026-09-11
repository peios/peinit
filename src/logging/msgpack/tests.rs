use crate::ids::JobIdAllocator;
use crate::logging::{LogStream, ServiceLogRecord};

use super::{
    encode_eventd_log_record, encode_eventd_log_records_into, encoded_eventd_log_record_len,
    eventd_log_batch_prefix_len,
};

#[test]
fn encodes_eventd_log_record_as_msgpack_map() {
    let job_id = JobIdAllocator::new()
        .allocate_batch(1, 1_000_000_000)
        .expect("job id")[0];
    let record = ServiceLogRecord::new("app", LogStream::Stderr, "failed", 42, Some(job_id));

    let bytes = encode_eventd_log_record(&record);

    assert_eq!(bytes.len(), encoded_eventd_log_record_len(&record));
    assert_eq!(bytes[0], 0x85);
    assert!(
        bytes
            .windows(b"origin".len())
            .any(|window| window == b"origin")
    );
    assert!(
        bytes
            .windows(b"is_error".len())
            .any(|window| window == b"is_error")
    );
    assert!(bytes.contains(&0xc3));
    assert!(
        bytes
            .windows(b"failed".len())
            .any(|window| window == b"failed")
    );
    assert!(
        bytes
            .windows(job_id.as_bytes().len())
            .any(|window| window == job_id.as_bytes())
    );
}

/// TRM §11.4: the record map has four or five entries depending on whether a
/// job identifier applies, and `job_id` is omitted — not sent as nil or as an
/// empty bin — when there is none. The same record with and without a job is
/// encoded and each is decoded key by key, so the test fails if the key were
/// written with a placeholder value, if the header still said five, or if the
/// five-entry form lost its key.
#[test]
fn a_record_without_a_job_id_is_a_four_entry_map() {
    let job_id = JobIdAllocator::new()
        .allocate_batch(1, 1_000_000_000)
        .expect("job id")[0];
    let without = encode_eventd_log_record(&ServiceLogRecord::new(
        "app",
        LogStream::Stdout,
        "line",
        7,
        None,
    ));
    let with = encode_eventd_log_record(&ServiceLogRecord::new(
        "app",
        LogStream::Stdout,
        "line",
        7,
        Some(job_id),
    ));

    assert_eq!(without[0], 0x84, "no job: a fixmap of four entries");
    assert_eq!(
        map_keys(&without),
        vec!["origin", "is_error", "message", "timestamp"],
        "and job_id is absent rather than present with an empty value",
    );
    assert_eq!(with[0], 0x85, "a job: a fixmap of five entries");
    assert_eq!(
        map_keys(&with),
        vec!["origin", "is_error", "message", "timestamp", "job_id"],
    );
    // The only difference between the two encodings is the job_id pair.
    assert_eq!(
        with.len() - without.len(),
        1 + "job_id".len() + 2 + job_id.as_bytes().len(),
    );
}

/// The keys of one encoded record, in order. Decodes exactly the value types
/// the encoder writes and panics on anything else, so a stray value type
/// cannot be skipped over silently.
fn map_keys(bytes: &[u8]) -> Vec<String> {
    fn str_at(bytes: &[u8], at: &mut usize) -> String {
        let tag = bytes[*at];
        let (len, header) = match tag {
            0xa0..=0xbf => ((tag & 0x1f) as usize, 1),
            0xd9 => (bytes[*at + 1] as usize, 2),
            _ => panic!("expected a string at {at}, found 0x{tag:02x}"),
        };
        let start = *at + header;
        *at = start + len;
        String::from_utf8(bytes[start..start + len].to_vec()).expect("utf-8 key")
    }
    fn skip_value(bytes: &[u8], at: &mut usize) {
        let tag = bytes[*at];
        *at += match tag {
            0x00..=0x7f | 0xc2 | 0xc3 => 1,
            0xcc => 2,
            0xcd => 3,
            0xce => 5,
            0xcf => 9,
            0xa0..=0xbf => 1 + (tag & 0x1f) as usize,
            0xd9 => 2 + bytes[*at + 1] as usize,
            0xc4 => 2 + bytes[*at + 1] as usize,
            _ => panic!("unexpected value type 0x{tag:02x} at {at}"),
        };
    }
    assert_eq!(bytes[0] & 0xf0, 0x80, "a record is a fixmap");
    let entries = (bytes[0] & 0x0f) as usize;
    let mut at = 1;
    let mut keys = Vec::with_capacity(entries);
    for _ in 0..entries {
        keys.push(str_at(bytes, &mut at));
        skip_value(bytes, &mut at);
    }
    assert_eq!(at, bytes.len(), "nothing follows the last entry");
    keys
}

#[test]
fn encodes_record_batches_as_one_msgpack_array() {
    let records = [
        ServiceLogRecord::new("app", LogStream::Stdout, "one", 1, None),
        ServiceLogRecord::new("app", LogStream::Stderr, "two", 2, None),
    ];
    let mut bytes = Vec::new();

    encode_eventd_log_records_into(&mut bytes, &records);

    assert_eq!(bytes[0], 0x92);
    assert_eq!(
        bytes.len(),
        1 + records
            .iter()
            .map(encoded_eventd_log_record_len)
            .sum::<usize>()
    );
}

#[test]
fn selects_the_largest_prefix_within_the_datagram_ceiling() {
    let records = [
        ServiceLogRecord::new("app", LogStream::Stdout, "one", 1, None),
        ServiceLogRecord::new("app", LogStream::Stdout, "two", 2, None),
        ServiceLogRecord::new("app", LogStream::Stdout, "three", 3, None),
    ];
    let first_two_bytes = 1 + records[..2]
        .iter()
        .map(encoded_eventd_log_record_len)
        .sum::<usize>();

    assert_eq!(
        eventd_log_batch_prefix_len(records.iter(), first_two_bytes),
        2
    );
    assert_eq!(
        eventd_log_batch_prefix_len(records.iter(), first_two_bytes - 1),
        1
    );
    assert_eq!(eventd_log_batch_prefix_len(records.iter(), 1), 0);
}
