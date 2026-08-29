//! Linux implementations of the submitted-job boundaries: preparing a job
//! identity from the kernel's token facilities, minting and evaluating job
//! descriptors, and wrapping a prepared token for the launch.

mod identity;
mod security;

pub use identity::LinuxJobIdentityProvider;
pub(in crate::boundary) use identity::materialize_linux_prepared_token;
