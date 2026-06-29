#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutableCommandParseError {
    Empty,
    UnclosedDoubleQuote,
    RelativeExecutable { executable: String },
}

pub(crate) fn parse_executable_command(
    command: &str,
) -> Result<Vec<String>, ExecutableCommandParseError> {
    let argv = split_command_argv(command)?;
    let Some(executable) = argv.first() else {
        return Err(ExecutableCommandParseError::Empty);
    };
    if !executable.starts_with('/') {
        return Err(ExecutableCommandParseError::RelativeExecutable {
            executable: executable.clone(),
        });
    }
    Ok(argv)
}

fn split_command_argv(command: &str) -> Result<Vec<String>, ExecutableCommandParseError> {
    let mut argv = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut started = false;

    for ch in command.chars() {
        if is_ascii_command_whitespace(ch) && !in_quote {
            if started {
                argv.push(std::mem::take(&mut current));
                started = false;
            }
            continue;
        }
        if ch == '"' {
            in_quote = !in_quote;
            started = true;
            continue;
        }
        current.push(ch);
        started = true;
    }

    if in_quote {
        return Err(ExecutableCommandParseError::UnclosedDoubleQuote);
    }
    if started {
        argv.push(current);
    }
    if argv.is_empty() {
        return Err(ExecutableCommandParseError::Empty);
    }
    Ok(argv)
}

fn is_ascii_command_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{000c}' | '\u{000b}')
}

#[cfg(test)]
mod tests;
