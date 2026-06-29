pub(super) fn decode_satisfied_bytes(bytes: &[u8], expected_count: usize) -> Vec<bool> {
    if bytes.len() != 4 + expected_count {
        return vec![false; expected_count];
    }
    let Ok(count_bytes) = <[u8; 4]>::try_from(&bytes[..4]) else {
        return vec![false; expected_count];
    };
    let count = u32::from_le_bytes(count_bytes);
    if count as usize != expected_count {
        return vec![false; expected_count];
    }
    bytes[4..].iter().map(|byte| *byte == 1).collect()
}

#[cfg(test)]
mod tests {
    use super::decode_satisfied_bytes;

    #[test]
    fn result_decode_fails_closed_on_length_or_count_mismatch() {
        assert_eq!(decode_satisfied_bytes(&[1, 0, 0], 2), vec![false, false]);
        assert_eq!(
            decode_satisfied_bytes(&[3, 0, 0, 0, 1, 1], 2),
            vec![false, false],
        );
        assert_eq!(
            decode_satisfied_bytes(&[2, 0, 0, 0, 1, 0], 2),
            vec![true, false],
        );
    }
}
