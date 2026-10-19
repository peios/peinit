//! Peios PID 1 service manager and control clients.
//!
//! The public crate surface is deliberately small: the binary entrypoint lives in
//! `init`, and `client` is what a program that manages services from outside
//! can use. Internal modules remain public for compatibility but are hidden from
//! generated API documentation. Within the crate
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
pub mod capi;
#[doc(hidden)]
pub mod cli;
pub mod client;
/// The console line format: the tag column, its colours, and the stage
/// banners. Public because `ConsoleTag` appears in the `InitPlatform` trait,
/// which an out-of-crate platform implements; `pub` rather than crate-private
/// because Phase 1 and the runtime reach the console by different paths and
/// both have to render identically.
#[doc(hidden)]
pub mod console_style;
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
pub mod jobs;
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
pub mod provisioning;
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
pub mod submitted;
#[doc(hidden)]
pub mod supervisor;
#[doc(hidden)]
pub mod timer;
