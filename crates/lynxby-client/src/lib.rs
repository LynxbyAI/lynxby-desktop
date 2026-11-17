//! Protocol client for the Lynxby desktop application.
//!
//! Everything here is headless: the UI crate owns pixels, this crate owns the
//! conversation with the deployment. That split is what makes the interesting
//! parts testable without a window.

pub mod api;
pub mod composer;
pub mod config;
pub mod error;
mod http;
pub mod oauth;
pub mod pkce;
pub mod signin;
pub mod sse;
pub mod store;
pub mod types;

pub use api::ApiClient;
pub use config::{Config, Environment};
pub use error::Error;
pub use store::{Session, Store};

/// Sent on every request so the deployment can tell desktop traffic apart in
/// the audit trail.
pub const USER_AGENT: &str = concat!("LynxbyDesktop/", env!("CARGO_PKG_VERSION"));
