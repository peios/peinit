use super::{ExecutableCommandParseError, parse_executable_command};

#[test]
fn parses_whitespace_and_double_quote_grouping() {
    assert_eq!(
        parse_executable_command(r#"/bin/reload --name="hello world" "" 'literal'"#)
            .expect("parse"),
        vec!["/bin/reload", "--name=hello world", "", "'literal'"],
    );
}

#[test]
fn treats_form_feed_and_vertical_tab_as_command_whitespace() {
    assert_eq!(
        parse_executable_command("/bin/hook\u{000c}one\u{000b}two").expect("parse"),
        vec!["/bin/hook", "one", "two"],
    );
}

#[test]
fn rejects_empty_unclosed_and_relative_commands() {
    assert_eq!(
        parse_executable_command(" \t\n").expect_err("empty"),
        ExecutableCommandParseError::Empty,
    );
    assert_eq!(
        parse_executable_command(r#"/bin/reload "unterminated"#).expect_err("unclosed"),
        ExecutableCommandParseError::UnclosedDoubleQuote,
    );
    assert_eq!(
        parse_executable_command("reload").expect_err("relative"),
        ExecutableCommandParseError::RelativeExecutable {
            executable: "reload".to_string(),
        },
    );
}
