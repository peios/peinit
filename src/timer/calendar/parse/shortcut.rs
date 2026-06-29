use std::borrow::Cow;

pub(super) fn expand_shortcut(expression: &str) -> (Cow<'_, str>, bool) {
    let mut tokens = expression.split_whitespace();
    let Some(first) = tokens.next() else {
        return (Cow::Borrowed(expression), false);
    };
    let Some(expanded) = shortcut_expression(first) else {
        return (Cow::Borrowed(expression), false);
    };

    match (tokens.next(), tokens.next()) {
        (None, None) => (Cow::Borrowed(expanded), true),
        (Some(timezone), None) => (Cow::Owned(format!("{expanded} {timezone}")), true),
        _ => (Cow::Borrowed(expression), false),
    }
}

fn shortcut_expression(shortcut: &str) -> Option<&'static str> {
    match shortcut.to_ascii_lowercase().as_str() {
        "minutely" => Some("*-*-* *:*:00"),
        "hourly" => Some("*-*-* *:00:00"),
        "daily" => Some("*-*-* 00:00:00"),
        "weekly" => Some("Mon *-*-* 00:00:00"),
        "monthly" => Some("*-*-01 00:00:00"),
        "quarterly" => Some("*-01,04,07,10-01 00:00:00"),
        "semiannually" => Some("*-01,07-01 00:00:00"),
        "annually" | "yearly" => Some("*-01-01 00:00:00"),
        _ => None,
    }
}
