#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TimerLastRunStorage {
    SingleTimer,
    PerTrigger,
}

pub const LAST_TIMER_RUN_VALUE_NAME: &[u8] = b"LastTimerRun";
pub const TIMER_STATE_SUBKEY_NAME: &str = "TimerState";

pub fn encode_timer_schedule_value_name(schedule: &str) -> String {
    let mut encoded = String::with_capacity(schedule.len());
    for byte in schedule.bytes() {
        if is_value_name_passthrough(byte) {
            encoded.push(byte as char);
        } else {
            push_percent_encoded(byte, &mut encoded);
        }
    }
    encoded
}

fn is_value_name_passthrough(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
}

fn push_percent_encoded(byte: u8, output: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    output.push('%');
    output.push(HEX[(byte >> 4) as usize] as char);
    output.push(HEX[(byte & 0x0F) as usize] as char);
}

#[cfg(test)]
mod tests {
    use super::encode_timer_schedule_value_name;

    #[test]
    fn normative_schedule_example_encodes_exactly() {
        assert_eq!(
            encode_timer_schedule_value_name("*-*-* 02:00:00"),
            "%2A-%2A-%2A%2002%3A00%3A00",
        );
    }

    #[test]
    fn value_name_safe_characters_pass_through() {
        assert_eq!(
            encode_timer_schedule_value_name("daily_v2.timer-01"),
            "daily_v2.timer-01",
        );
    }

    #[test]
    fn non_ascii_utf8_bytes_are_percent_encoded() {
        assert_eq!(encode_timer_schedule_value_name("daily-µ"), "daily-%C2%B5");
    }

    #[test]
    fn encoding_preserves_distinct_schedules() {
        assert_ne!(
            encode_timer_schedule_value_name("Europe/London"),
            encode_timer_schedule_value_name("Europe-London"),
        );
        assert_eq!(
            encode_timer_schedule_value_name("Europe/London"),
            "Europe%2FLondon",
        );
    }
}
