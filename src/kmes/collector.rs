//! Collecting one turn's events, asking the emission policy first.

use crate::boundary::{BoundaryError, EventTier, EventTimeProjection, KmesEvent};

use super::types::tier_of;

/// The events of one turn, in order, built only if the emission policy has
/// their type switched on (PGSS §6.9).
///
/// Every non-essential event is offered with [`push`](Self::push), which
/// consults the policy before the payload is built: a switched-off event
/// costs its type's lookup and nothing more. An `essential` event never
/// consults it. The collector also carries the clocks read for the turn, so
/// a monotonic instant can be written as the wall-clock time it was.
pub struct EventCollector<'a> {
    enabled: &'a dyn Fn(&str, EventTier) -> bool,
    time: EventTimeProjection,
    events: Vec<KmesEvent>,
}

fn always(_: &str, _: EventTier) -> bool {
    true
}

impl<'a> EventCollector<'a> {
    /// A collector asking `enabled` whether a type is switched on.
    pub fn new(enabled: &'a dyn Fn(&str, EventTier) -> bool, time: EventTimeProjection) -> Self {
        Self {
            enabled,
            time,
            events: Vec::new(),
        }
    }

    /// A collector that writes every event, as with no policy at all.
    pub fn everything(time: EventTimeProjection) -> EventCollector<'static> {
        EventCollector::new(&always, time)
    }

    /// Whether `event_type` would be written: always for an essential type,
    /// otherwise as the policy says.
    pub fn enabled(&self, event_type: &str) -> bool {
        match tier_of(event_type) {
            EventTier::Essential => true,
            tier => (self.enabled)(event_type, tier),
        }
    }

    /// The clocks read for this turn.
    pub fn time(&self) -> EventTimeProjection {
        self.time
    }

    /// Build and keep an event of `event_type` if the policy has it on.
    pub fn push(
        &mut self,
        event_type: &'static str,
        build: impl FnOnce(EventTimeProjection) -> Result<KmesEvent, BoundaryError>,
    ) -> Result<(), BoundaryError> {
        if !self.enabled(event_type) {
            return Ok(());
        }
        let event = build(self.time)?;
        debug_assert_eq!(event.event_type, event_type);
        self.events.push(event);
        Ok(())
    }

    pub fn events(&self) -> &[KmesEvent] {
        &self.events
    }

    pub fn into_events(self) -> Vec<KmesEvent> {
        self.events
    }
}
