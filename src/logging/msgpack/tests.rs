use crate::ids::JobIdAllocator;
use crate::logging::{LogStream, ServiceLogRecord};

use super::encode_eventd_log_record;

#[test]
fn encodes_eventd_log_record_as_msgpack_map() {
    let job_id = JobIdAllocator::new()
        .allocate_batch(1, 1_000_000_000)
        .expect("job id")[0];
    let record = ServiceLogRecord::new("app", LogStream::Stderr, "failed", 42, Some(job_id));

    let bytes = encode_eventd_log_record(&record);

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
