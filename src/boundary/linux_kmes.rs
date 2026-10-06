use std::sync::OnceLock;

use peios::event::{self, EmitEntry, EventPolicy, Tier};

use super::{BoundaryError, EventTier, EventTimeProjection, KmesEvent, KmesEventSink};

#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxKmesEventSink;

impl LinuxKmesEventSink {
    pub fn new() -> Self {
        Self
    }
}

/// peinit's one view of the emission policy, opened on first use and kept
/// for the life of PID 1.
///
/// Opening needs no registry: until `Machine\Generic\Events` is readable
/// every decision is by tier, and libpeios watches the key so a committed
/// change applies to the next decision (PGSS §6.9). `None` only if the open
/// failed for want of memory, in which case every event is written, as
/// before the policy existed: losing the audit trail is worse than
/// over-filling it.
fn emission_policy() -> Option<&'static EventPolicy> {
    static POLICY: OnceLock<Option<EventPolicy>> = OnceLock::new();
    POLICY.get_or_init(|| EventPolicy::open().ok()).as_ref()
}

fn sdk_tier(tier: EventTier) -> Tier {
    match tier {
        EventTier::Essential => Tier::Essential,
        EventTier::Standard => Tier::Standard,
        EventTier::Verbose => Tier::Verbose,
        EventTier::Debug => Tier::Debug,
    }
}

impl KmesEventSink for LinuxKmesEventSink {
    fn emit_kmes_event(&mut self, event: &KmesEvent) -> Result<(), BoundaryError> {
        event::emit(&event.event_type, &event.payload).map_err(kmes_error)
    }

    fn emit_kmes_events(&mut self, events: &[KmesEvent]) -> Result<(), BoundaryError> {
        if events.is_empty() {
            return Ok(());
        }

        let entries: Vec<_> = events
            .iter()
            .map(|event| EmitEntry {
                event_type: event.event_type.as_str(),
                payload: event.payload.as_slice(),
            })
            .collect();

        event::emit_batch(&entries)
            .and_then(|emitted| {
                if emitted == entries.len() {
                    Ok(())
                } else {
                    Err(peios::Error::from_raw_os_error(libc::EIO))
                }
            })
            .map_err(kmes_error)
    }

    fn kmes_event_enabled(&self, event_type: &str, tier: EventTier) -> bool {
        if tier == EventTier::Essential {
            return true;
        }
        match emission_policy() {
            // An error is a malformed type, which is peinit's bug, not a
            // reason to lose the record.
            Some(policy) => policy.enabled(event_type, sdk_tier(tier)).unwrap_or(true),
            None => true,
        }
    }

    fn kmes_time_projection(&mut self) -> EventTimeProjection {
        let read = |clock| {
            let mut spec = libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            // SAFETY: `spec` is a valid, writable timespec for the call.
            let rc = unsafe { libc::clock_gettime(clock, &mut spec) };
            if rc != 0 {
                return 0;
            }
            u64::try_from(spec.tv_sec)
                .unwrap_or(0)
                .saturating_mul(1_000_000_000)
                .saturating_add(u64::try_from(spec.tv_nsec).unwrap_or(0))
        };
        EventTimeProjection::new(read(libc::CLOCK_MONOTONIC), read(libc::CLOCK_REALTIME))
    }
}

fn kmes_error(error: peios::Error) -> BoundaryError {
    match error.raw_os_error() {
        Some(errno) => BoundaryError::KmesRefused {
            errno,
            message: error.to_string(),
        },
        None => BoundaryError::Kmes(error.to_string()),
    }
}
