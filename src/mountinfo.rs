pub(crate) fn parse_mountinfo_mount_points(contents: &str) -> Result<Vec<String>, String> {
    contents
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            parse_mountinfo_mount_point(line)
                .map_err(|message| format!("invalid mountinfo line {}: {message}", index + 1))
        })
        .collect()
}

pub(crate) fn mountinfo_contains_mount_point(
    contents: &str,
    mount_point: &str,
) -> Result<bool, String> {
    Ok(parse_mountinfo_mount_points(contents)?
        .iter()
        .any(|entry| entry == mount_point))
}

fn parse_mountinfo_mount_point(line: &str) -> Result<String, String> {
    let mut fields = line.split(' ');
    for field_index in 0..4 {
        fields
            .next()
            .ok_or_else(|| format!("missing field {}", field_index + 1))?;
    }
    let mount_point = fields
        .next()
        .ok_or_else(|| "missing mount point field".to_string())?;
    decode_mountinfo_escapes(mount_point)
}

fn decode_mountinfo_escapes(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'\\' {
            output.push(bytes[index]);
            index += 1;
            continue;
        }

        let Some(octal) = bytes.get(index + 1..index + 4) else {
            return Err("truncated escape in mount point".to_string());
        };
        let mut decoded = 0u8;
        for digit in octal {
            if !(b'0'..=b'7').contains(digit) {
                return Err("non-octal escape in mount point".to_string());
            }
            decoded = decoded.saturating_mul(8).saturating_add(digit - b'0');
        }
        output.push(decoded);
        index += 4;
    }

    String::from_utf8(output).map_err(|error| format!("mount point is not UTF-8: {error}"))
}

#[cfg(test)]
mod tests;
