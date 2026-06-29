use super::{NotifyField, NotifyParseError, parse_notify_message};

#[test]
fn parses_supported_newline_separated_fields_and_ignores_unknown() {
    let message = parse_notify_message(
        b"READY=1\nSTATUS=Listening\nWATCHDOG=1\nMAINPID=1234\n\nRELOADING=1\n",
    )
    .expect("parse notify");

    assert_eq!(
        message.fields,
        vec![
            NotifyField::Ready,
            NotifyField::Status("Listening".to_string()),
            NotifyField::Watchdog,
            NotifyField::Reloading,
        ],
    );
}

#[test]
fn rejects_whole_datagram_on_malformed_line() {
    assert_eq!(
        parse_notify_message(b"STATUS=ok\nbroken\nREADY=1").expect_err("malformed"),
        NotifyParseError::MalformedLine { line_index: 1 },
    );
    assert_eq!(
        parse_notify_message(b"=value").expect_err("empty key"),
        NotifyParseError::MalformedLine { line_index: 0 },
    );
}
