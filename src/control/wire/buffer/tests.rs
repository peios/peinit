use super::{ControlBufferConsumeError, ControlConnectionBuffer};
use crate::control::wire::{
    ControlFrameDecision, ControlFrameRejectReason, control_frame_decision,
};

#[test]
fn buffers_partial_reads_until_complete_frame() {
    let mut buffer = ControlConnectionBuffer::new();

    buffer.append(br#"{"command":"shutdown""#);
    assert_eq!(buffer.frame_decision(128), ControlFrameDecision::Incomplete);
    buffer.append(br#","type":"reboot"}"#);
    assert_eq!(buffer.frame_decision(128), ControlFrameDecision::Incomplete);
    buffer.append(b"\n{\"command\":\"list\"}\n");

    let ControlFrameDecision::Complete { body, consumed } = buffer.frame_decision(128) else {
        panic!("expected complete frame");
    };
    assert_eq!(body, br#"{"command":"shutdown","type":"reboot"}"#);
    buffer.consume(consumed).expect("consume frame");
    assert_eq!(buffer.as_slice(), b"{\"command\":\"list\"}\n");
}

#[test]
fn preserves_buffer_after_reject_decision() {
    let mut buffer = ControlConnectionBuffer::new();
    buffer.append(b"abcdef");

    assert_eq!(
        buffer.frame_decision(5),
        ControlFrameDecision::Reject {
            reason: ControlFrameRejectReason::RequestTooLarge,
        },
    );
    assert_eq!(buffer.as_slice(), b"abcdef");
    assert_eq!(
        control_frame_decision(buffer.as_slice(), 5),
        buffer.frame_decision(5),
    );
}

#[test]
fn rejects_overconsumption_and_can_clear() {
    let mut buffer = ControlConnectionBuffer::new();
    buffer.append(b"abc");

    assert_eq!(
        buffer.consume(4).expect_err("overconsume"),
        ControlBufferConsumeError::TooMany {
            requested: 4,
            available: 3,
        },
    );
    assert_eq!(buffer.len(), 3);
    buffer.clear();
    assert!(buffer.is_empty());
}
