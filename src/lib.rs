//! Small replacement spine for peinit.
//!
//! This crate is intentionally separate from `peinit` while the production path is
//! rebuilt. The rule is that production orchestration owns IDs, timestamps, and
//! state transitions; test doubles belong only behind explicit external
//! boundaries.
//!
//! The public crate surface is deliberately small: the binary entrypoint lives in
//! `init`, while service-manager internals stay crate-private. Within the crate
//! the architecture is layered as:
//!
//! - data/state models: `service`, `operation`, `job`, `timer`, `logging`
//! - pure transitions: `boot`, `control`, `execution`, `shutdown`, `notify`
//! - orchestration: `supervisor`
//! - external effects: `boundary`, `runtime`, and Linux-specific `init`
//!
//! State transitions are expected to be atomic at their public boundary. When a
//! transition touches several stores it clones the affected state, applies the
//! full change, and commits only after every step succeeds.

#[doc(hidden)]
pub mod boot;
#[doc(hidden)]
pub mod boundary;
#[doc(hidden)]
pub mod control;
#[doc(hidden)]
pub mod execution;
#[doc(hidden)]
pub mod fd_store;
#[doc(hidden)]
pub mod ids;
pub mod init;
#[doc(hidden)]
pub mod job;
#[doc(hidden)]
#[cfg(feature = "peios-boundary")]
pub mod kmes;
#[doc(hidden)]
pub mod logging;
mod mountinfo;
#[doc(hidden)]
pub mod notify;
#[doc(hidden)]
pub mod operation;
#[doc(hidden)]
pub mod registry;
#[doc(hidden)]
pub mod runtime;
#[doc(hidden)]
pub mod security;
#[doc(hidden)]
pub mod service;
#[doc(hidden)]
pub mod shutdown;
#[doc(hidden)]
pub mod supervisor;
#[doc(hidden)]
pub mod timer;
