use crate::boundary::BoundaryError;

const NANOS_PER_SEC: u64 = 1_000_000_000;

pub trait TimerJitterRandom {
    fn random_u64(&mut self) -> Result<u64, BoundaryError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JitteredTimerDeadline {
    pub scheduled_ns: u64,
    pub armed_ns: u64,
    pub delay_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerJitterError {
    Random(BoundaryError),
    JitterWindowOverflow { jitter_secs: u64 },
    DeadlineOverflow { scheduled_ns: u64, delay_secs: u64 },
}

pub fn apply_timer_jitter<R>(
    scheduled_ns: u64,
    jitter_secs: u64,
    random: &mut R,
) -> Result<JitteredTimerDeadline, TimerJitterError>
where
    R: TimerJitterRandom + ?Sized,
{
    let delay_secs = timer_jitter_delay_secs(jitter_secs, random)?;
    let delay_ns =
        delay_secs
            .checked_mul(NANOS_PER_SEC)
            .ok_or(TimerJitterError::DeadlineOverflow {
                scheduled_ns,
                delay_secs,
            })?;
    let armed_ns =
        scheduled_ns
            .checked_add(delay_ns)
            .ok_or(TimerJitterError::DeadlineOverflow {
                scheduled_ns,
                delay_secs,
            })?;

    Ok(JitteredTimerDeadline {
        scheduled_ns,
        armed_ns,
        delay_secs,
    })
}

fn timer_jitter_delay_secs<R>(jitter_secs: u64, random: &mut R) -> Result<u64, TimerJitterError>
where
    R: TimerJitterRandom + ?Sized,
{
    if jitter_secs == 0 {
        return Ok(0);
    }

    let range = jitter_secs
        .checked_add(1)
        .ok_or(TimerJitterError::JitterWindowOverflow { jitter_secs })?;
    let acceptance_zone = u64::MAX - (u64::MAX % range);
    loop {
        let sample = random.random_u64().map_err(TimerJitterError::Random)?;
        if sample < acceptance_zone {
            return Ok(sample % range);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use crate::boundary::BoundaryError;

    use super::{JitteredTimerDeadline, TimerJitterError, TimerJitterRandom, apply_timer_jitter};

    #[test]
    fn zero_jitter_keeps_scheduled_deadline_and_does_not_read_random() {
        let mut random = ScriptedRandom::default();

        assert_eq!(
            apply_timer_jitter(1_000, 0, &mut random).expect("jitter"),
            JitteredTimerDeadline {
                scheduled_ns: 1_000,
                armed_ns: 1_000,
                delay_secs: 0,
            },
        );
        assert_eq!(random.calls, 0);
    }

    #[test]
    fn jitter_delay_is_inclusive_of_zero_and_max_seconds() {
        let mut random = ScriptedRandom::from_values([Ok(0), Ok(5)]);

        assert_eq!(
            apply_timer_jitter(1_000, 5, &mut random)
                .expect("zero delay")
                .armed_ns,
            1_000,
        );
        assert_eq!(
            apply_timer_jitter(1_000, 5, &mut random)
                .expect("max delay")
                .armed_ns,
            5_000_001_000,
        );
    }

    #[test]
    fn biased_tail_samples_are_rejected() {
        let mut random = ScriptedRandom::from_values([Ok(u64::MAX), Ok(1)]);

        assert_eq!(
            apply_timer_jitter(1_000, 1, &mut random)
                .expect("retry")
                .delay_secs,
            1,
        );
        assert_eq!(random.calls, 2);
    }

    #[test]
    fn random_failures_are_reported() {
        let mut random =
            ScriptedRandom::from_values([Err(BoundaryError::Timer("entropy unavailable".into()))]);

        assert!(matches!(
            apply_timer_jitter(1_000, 1, &mut random).expect_err("random failure"),
            TimerJitterError::Random(BoundaryError::Timer(message))
                if message == "entropy unavailable"
        ));
    }

    #[test]
    fn overflowing_jitter_window_is_rejected() {
        let mut random = ScriptedRandom::default();

        assert_eq!(
            apply_timer_jitter(1_000, u64::MAX, &mut random).expect_err("window overflow"),
            TimerJitterError::JitterWindowOverflow {
                jitter_secs: u64::MAX,
            },
        );
    }

    #[test]
    fn overflowing_armed_deadline_is_rejected() {
        let mut random = ScriptedRandom::from_values([Ok(1)]);

        assert_eq!(
            apply_timer_jitter(u64::MAX - 10, 1, &mut random).expect_err("deadline overflow"),
            TimerJitterError::DeadlineOverflow {
                scheduled_ns: u64::MAX - 10,
                delay_secs: 1,
            },
        );
    }

    #[derive(Debug, Default)]
    struct ScriptedRandom {
        values: VecDeque<Result<u64, BoundaryError>>,
        calls: usize,
    }

    impl ScriptedRandom {
        fn from_values(values: impl IntoIterator<Item = Result<u64, BoundaryError>>) -> Self {
            Self {
                values: values.into_iter().collect(),
                calls: 0,
            }
        }
    }

    impl TimerJitterRandom for ScriptedRandom {
        fn random_u64(&mut self) -> Result<u64, BoundaryError> {
            self.calls += 1;
            self.values
                .pop_front()
                .expect("scripted random value should exist")
        }
    }
}
