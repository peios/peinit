use super::BoundaryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KmesEvent {
    pub event_type: String,
    pub payload: Vec<u8>,
}

impl KmesEvent {
    pub fn new(event_type: impl Into<String>, payload: Vec<u8>) -> Self {
        Self {
            event_type: event_type.into(),
            payload,
        }
    }
}

/// An event type's tier (PGSS §6.8): what the emission policy decides when
/// nothing in the registry says otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventTier {
    /// Always written; the policy is never consulted.
    Essential,
    /// On unless the policy switches it off.
    Standard,
    /// Off unless the policy switches it on.
    Verbose,
    /// Off unless the policy switches it on.
    Debug,
}

/// The wall clock and the monotonic clock read together, so a monotonic
/// instant peinit recorded can be written into an event as the wall-clock
/// time it was (`uint.time`, PGSS §6.5).
///
/// The projection assumes the wall clock has not been stepped between the
/// instant and the reading: an instant from before a step is reported on
/// the clock as it now stands.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EventTimeProjection {
    pub monotonic_now_ns: u64,
    pub realtime_now_ns: u64,
}

impl EventTimeProjection {
    pub const fn new(monotonic_now_ns: u64, realtime_now_ns: u64) -> Self {
        Self {
            monotonic_now_ns,
            realtime_now_ns,
        }
    }

    /// The wall-clock time, in nanoseconds since the Unix epoch, of a
    /// `CLOCK_MONOTONIC` instant.
    pub fn realtime_ns(self, monotonic_ns: u64) -> u64 {
        if monotonic_ns <= self.monotonic_now_ns {
            self.realtime_now_ns
                .saturating_sub(self.monotonic_now_ns - monotonic_ns)
        } else {
            self.realtime_now_ns
                .saturating_add(monotonic_ns - self.monotonic_now_ns)
        }
    }
}

pub trait KmesEventSink {
    fn emit_kmes_event(&mut self, event: &KmesEvent) -> Result<(), BoundaryError>;

    fn emit_kmes_events(&mut self, events: &[KmesEvent]) -> Result<(), BoundaryError> {
        for event in events {
            self.emit_kmes_event(event)?;
        }
        Ok(())
    }

    /// Whether the emission policy (`Machine\Generic\Events`, PGSS §6.9)
    /// has `event_type` switched on. Asked before an event's payload is
    /// built, and never for an `essential` one. A sink with no policy to
    /// consult writes everything.
    fn kmes_event_enabled(&self, _event_type: &str, _tier: EventTier) -> bool {
        true
    }

    /// The clocks, read now, for writing monotonic instants as wall-clock
    /// times. A sink with no clocks projects each instant onto itself.
    fn kmes_time_projection(&mut self) -> EventTimeProjection {
        EventTimeProjection::default()
    }
}
