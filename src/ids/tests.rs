use super::{IdAllocationError, JobIdAllocator, OperationId, OperationIdAllocator};

const OBSERVED_AT_NS: u64 = 1_717_171_717_123_456_789;

#[test]
fn operation_allocator_allocates_distinct_uuidv7_ids() {
    let mut allocator = OperationIdAllocator::new();

    let ids = allocator.allocate_batch(2, OBSERVED_AT_NS).expect("ids");

    assert_eq!(allocator.next_sequence(), 2);
    assert_ne!(ids[0], ids[1]);
    assert_eq!(ids[0].as_bytes()[6] >> 4, 0x7);
    assert_eq!(ids[0].as_bytes()[8] & 0b1100_0000, 0b1000_0000);
}

/// A `bin.guid` field carries a GUID in its PCDS binary form, whose first
/// three fields are little-endian; read back that way, it is the same
/// canonical text the control channel shows.
#[test]
fn the_pcds_form_reads_back_as_the_canonical_text() {
    let id = JobIdAllocator::new()
        .allocate_batch(1, OBSERVED_AT_NS)
        .expect("ids")[0];
    let b = id.as_guid_bytes();
    let read_back = format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{}",
        u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        u16::from_le_bytes([b[4], b[5]]),
        u16::from_le_bytes([b[6], b[7]]),
        b[8],
        b[9],
        b[10..].iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
    );
    assert_eq!(read_back, id.to_string());
    assert_eq!(b[8..], id.as_bytes()[8..], "the last eight bytes are as they are");
}

#[test]
fn job_allocator_is_independent_from_operation_allocator() {
    let mut operations = OperationIdAllocator::new();
    let mut jobs = JobIdAllocator::new();

    let operation_id = operations
        .allocate_batch(1, OBSERVED_AT_NS)
        .expect("operation ids")[0];
    let job_id = jobs.allocate_batch(1, OBSERVED_AT_NS).expect("job ids")[0];

    assert_eq!(operation_id.as_bytes(), job_id.as_bytes());
    assert_eq!(operations.next_sequence(), 1);
    assert_eq!(jobs.next_sequence(), 1);
}

#[test]
fn allocator_does_not_advance_on_sequence_exhaustion() {
    let mut allocator = OperationIdAllocator::with_next_sequence(u64::MAX);

    let err = allocator
        .allocate_batch(1, OBSERVED_AT_NS)
        .expect_err("sequence exhaustion");

    assert_eq!(
        err,
        IdAllocationError::SequenceExhausted {
            next_sequence: u64::MAX,
            count: 1,
        }
    );
    assert_eq!(allocator.next_sequence(), u64::MAX);
}

#[test]
fn operation_id_parses_canonical_uuidv7_string() {
    let id = OperationIdAllocator::new()
        .allocate_batch(1, OBSERVED_AT_NS)
        .expect("operation id")[0];

    let parsed = id
        .to_canonical_string()
        .parse::<OperationId>()
        .expect("parse operation id");

    assert_eq!(parsed, id);
}

#[test]
fn operation_id_rejects_invalid_canonical_string() {
    assert!("not-a-uuid".parse::<OperationId>().is_err());
    assert!(
        "018f32bb-2d83-6000-8000-000000001115"
            .parse::<OperationId>()
            .is_err()
    );
    assert!(
        "018f32bb-2d83-7000-0000-000000001115"
            .parse::<OperationId>()
            .is_err()
    );
}
