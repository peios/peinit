//! The jobs channel (PSPU §7): the sequenced-packet socket a submitter
//! reaches peinit on, its wire vocabulary, and the per-connection state.
//!
//! This module is the transport and the framing. What a submission *means*
//! — identity, quota, the job itself — is `crate::submitted` and the
//! supervisor; this is the door.

pub mod client;
pub mod connection;
pub mod socket;
pub mod wire;
