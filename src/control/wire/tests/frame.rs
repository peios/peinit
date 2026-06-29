use super::support::*;

#[test]
fn extracts_complete_newline_delimited_frame() {
    assert_eq!(
        control_frame_decision(br#"{"command":"list"}"#, 64),
        ControlFrameDecision::Incomplete,
    );
    assert_eq!(
        control_frame_decision(b"{\"command\":\"list\"}\nnext", 64),
        ControlFrameDecision::Complete {
            body: br#"{"command":"list"}"#.to_vec(),
            consumed: br#"{"command":"list"}"#.len() + 1,
        },
    );
}

#[test]
fn rejects_empty_or_oversized_frame() {
    assert_eq!(
        control_frame_decision(b"\n", 64),
        ControlFrameDecision::Reject {
            reason: ControlFrameRejectReason::MalformedRequest,
        },
    );
    assert_eq!(
        control_frame_decision(b"abcdef\n", 5),
        ControlFrameDecision::Reject {
            reason: ControlFrameRejectReason::RequestTooLarge,
        },
    );
    assert_eq!(
        control_frame_decision(b"abcdef", 5),
        ControlFrameDecision::Reject {
            reason: ControlFrameRejectReason::RequestTooLarge,
        },
    );
}
