use crate::ids::{JobId, JobIdAllocator};
use crate::logging::{LogStream, ServiceLogRecord};

use super::{
    encode_eventd_log_record, encode_eventd_log_records_into, encoded_eventd_log_record_len,
    eventd_log_batch_prefix_len,
};

// Every one of the first three fields changes under PCDS's mixed-endian
// layout. Keep the wire oracle literal rather than deriving it with the
// conversion used by the encoder (PEI-1238, PSPU §3.7).
const JOB_ID_TEXT: &str = "01234567-89ab-7cde-8fab-0123456789ab";
const JOB_ID_PCDS: [u8; 16] = [
    0x67, 0x45, 0x23, 0x01, 0xab, 0x89, 0xde, 0x7c, 0x8f, 0xab, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
];

#[test]
fn encodes_eventd_log_record_as_msgpack_map() {
    let job_id = JobId::parse_canonical_str(JOB_ID_TEXT).expect("job id");
    let record = ServiceLogRecord::new("app", LogStream::Stderr, "failed", 42, Some(job_id));

    let bytes = encode_eventd_log_record(&record);
    let mut at = 0;
    let fields = decode_record(&bytes, &mut at);

    assert_eq!(bytes.len(), encoded_eventd_log_record_len(&record));
    assert_eq!(at, bytes.len(), "nothing follows the last entry");
    assert_record_fields(&fields, &record, Some(&JOB_ID_PCDS));
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

#[derive(Debug, PartialEq, Eq)]
enum Value<'a> {
    Str(&'a str),
    Bool(bool),
    Uint(u64),
    Bin(&'a [u8]),
}

// Decode the actual MessagePack fields, rather than searching for a byte
// substring that could occur in a message or in the wrong field.
fn decode_value<'a>(bytes: &'a [u8], at: &mut usize) -> Value<'a> {
    let tag = bytes[*at];
    *at += 1;
    match tag {
        0x00..=0x7f => Value::Uint(u64::from(tag)),
        0xc2 | 0xc3 => Value::Bool(tag == 0xc3),
        0xcc..=0xcf => Value::Uint(read_uint(bytes, at, 1 << (tag - 0xcc))),
        0xa0..=0xbf | 0xd9..=0xdb | 0xc4..=0xc6 => {
            let (len, binary) = match tag {
                0xa0..=0xbf => ((tag & 0x1f) as usize, false),
                0xd9..=0xdb => (read_uint(bytes, at, 1 << (tag - 0xd9)) as usize, false),
                0xc4..=0xc6 => (read_uint(bytes, at, 1 << (tag - 0xc4)) as usize, true),
                _ => unreachable!(),
            };
            let value = &bytes[*at..*at + len];
            *at += len;
            if binary {
                Value::Bin(value)
            } else {
                Value::Str(std::str::from_utf8(value).expect("utf-8 string"))
            }
        }
        _ => panic!("unexpected value type 0x{tag:02x} at {}", *at - 1),
    }
}

fn read_uint(bytes: &[u8], at: &mut usize, len: usize) -> u64 {
    let value = bytes[*at..*at + len]
        .iter()
        .fold(0, |value, byte| (value << 8) | u64::from(*byte));
    *at += len;
    value
}

fn decode_record<'a>(bytes: &'a [u8], at: &mut usize) -> Vec<(&'a str, Value<'a>)> {
    let tag = bytes[*at];
    assert_eq!(tag & 0xf0, 0x80, "a record is a fixmap");
    *at += 1;
    (0..tag & 0x0f)
        .map(|_| {
            let Value::Str(key) = decode_value(bytes, at) else {
                panic!("a record key is a string");
            };
            (key, decode_value(bytes, at))
        })
        .collect()
}

fn map_keys(bytes: &[u8]) -> Vec<&str> {
    let mut at = 0;
    let fields = decode_record(bytes, &mut at);
    assert_eq!(at, bytes.len(), "nothing follows the last entry");
    fields.into_iter().map(|(key, _)| key).collect()
}

fn assert_record_fields(
    fields: &[(&str, Value<'_>)],
    record: &ServiceLogRecord,
    job_id_bytes: Option<&[u8; 16]>,
) {
    let mut expected = vec![
        ("origin", Value::Str(&record.origin)),
        ("is_error", Value::Bool(record.is_error)),
        ("message", Value::Str(&record.message)),
        ("timestamp", Value::Uint(record.timestamp_ns)),
    ];
    if let Some(bytes) = job_id_bytes {
        expected.push(("job_id", Value::Bin(bytes)));
    }
    assert_eq!(fields, expected, "record fields and wire value types");
    if let Some(job_id) = record.job_id {
        let Value::Bin(bytes) = &fields[4].1 else {
            panic!("job_id is binary");
        };
        // Read the PCDS fields as eventd does when rendering a GUID. This
        // roundtrip must recover the control channel's canonical job ID.
        let text = format!(
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{}",
            u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            u16::from_le_bytes(bytes[4..6].try_into().unwrap()),
            u16::from_le_bytes(bytes[6..8].try_into().unwrap()),
            bytes[8],
            bytes[9],
            bytes[10..]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        );
        assert_eq!(text, job_id.to_canonical_string());
    }
}

#[test]
fn encodes_record_batches_as_one_msgpack_array() {
    let second_id =
        JobId::parse_canonical_str("fedcba98-7654-7321-b012-3456789abcde").expect("second job id");
    let second_pcds = [
        0x98, 0xba, 0xdc, 0xfe, 0x54, 0x76, 0x21, 0x73, 0xb0, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
        0xde,
    ];
    let fixtures = [
        (
            ServiceLogRecord::new(
                format!("jobs/{JOB_ID_TEXT}"),
                LogStream::Stderr,
                "x".repeat(256),
                u64::MAX,
                Some(JobId::parse_canonical_str(JOB_ID_TEXT).expect("job id")),
            ),
            Some(&JOB_ID_PCDS),
        ),
        (
            ServiceLogRecord::new("app", LogStream::Stdout, "", 0, None),
            None,
        ),
        (
            ServiceLogRecord::new(
                "app/HealthCheck",
                LogStream::Stdout,
                "done",
                65536,
                Some(second_id),
            ),
            Some(&second_pcds),
        ),
    ];
    let mut bytes = vec![0xff; 1024];

    // Empty, single, mixed records and the fixarray/array16 boundary. Reuse
    // the output allocation so old bytes cannot survive a shorter batch.
    for count in [16, 3, 1, 0] {
        let records: Vec<_> = fixtures
            .iter()
            .cycle()
            .take(count)
            .map(|(record, _)| record.clone())
            .collect();
        encode_eventd_log_records_into(&mut bytes, &records);
        let mut at = 1;
        let decoded_count = match bytes[0] {
            0x90..=0x9f => usize::from(bytes[0] & 0x0f),
            0xdc => read_uint(&bytes, &mut at, 2) as usize,
            tag => panic!("expected an array, found 0x{tag:02x}"),
        };
        assert_eq!(decoded_count, count);
        assert_eq!(
            bytes.len(),
            at + records
                .iter()
                .map(encoded_eventd_log_record_len)
                .sum::<usize>(),
        );
        for (record, job_id_bytes) in fixtures.iter().cycle().take(count) {
            let start = at;
            let fields = decode_record(&bytes, &mut at);
            assert_eq!(at - start, encoded_eventd_log_record_len(record));
            assert_record_fields(&fields, record, *job_id_bytes);
            assert_eq!(&bytes[start..at], encode_eventd_log_record(record));
        }
        assert_eq!(at, bytes.len(), "nothing follows the final record");
        assert_eq!(
            eventd_log_batch_prefix_len(records.iter(), bytes.len()),
            count
        );
        if count > 0 {
            assert_eq!(
                eventd_log_batch_prefix_len(records.iter(), bytes.len() - 1),
                count - 1
            );
        }
    }
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
