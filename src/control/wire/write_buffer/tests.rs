use super::{ControlWriteBuffer, ControlWriteBufferError};

#[test]
fn queues_response_bytes_and_tracks_partial_progress() {
    let mut buffer = ControlWriteBuffer::new();

    buffer.enqueue(b"{\"status\":\"ok\"}\n");
    assert_eq!(buffer.pending_slice(), b"{\"status\":\"ok\"}\n");
    buffer.advance_written(5).expect("partial write");
    assert_eq!(buffer.pending_slice(), b"tus\":\"ok\"}\n");
    buffer.enqueue(b"{\"status\":\"error\"}\n");
    assert_eq!(
        buffer.pending_slice(),
        b"tus\":\"ok\"}\n{\"status\":\"error\"}\n",
    );
}

#[test]
fn clears_storage_after_all_bytes_are_written() {
    let mut buffer = ControlWriteBuffer::new();
    buffer.enqueue(b"abc");
    buffer.advance_written(3).expect("complete write");

    assert!(buffer.is_empty());
    assert_eq!(buffer.pending_slice(), b"");
    buffer.enqueue(b"next");
    assert_eq!(buffer.pending_slice(), b"next");
}

#[test]
fn rejects_advancing_past_pending_bytes() {
    let mut buffer = ControlWriteBuffer::new();
    buffer.enqueue(b"abc");

    assert_eq!(
        buffer.advance_written(4).expect_err("past end"),
        ControlWriteBufferError::AdvancePastEnd {
            written: 4,
            pending: 3,
        },
    );
    assert_eq!(buffer.pending_slice(), b"abc");
}

#[test]
fn clear_drops_pending_and_written_state() {
    let mut buffer = ControlWriteBuffer::new();
    buffer.enqueue(b"abc");
    buffer.advance_written(1).expect("partial write");
    buffer.clear();

    assert!(buffer.is_empty());
    buffer.enqueue(b"x");
    assert_eq!(buffer.pending_slice(), b"x");
}
