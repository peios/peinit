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
