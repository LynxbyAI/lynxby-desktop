//! One HTTP agent for the whole process.
//!
//! `http_status_as_error(false)` keeps the body of a 4xx: the deployment's
//! error shape is the interesting part, and ureq's status error would throw it
//! away. TLS roots come from the platform, so a ddev certificate trusted by the
//! system (mkcert) is trusted here too.

use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig};

use crate::USER_AGENT;

pub(crate) fn agent() -> ureq::Agent {
    let tls = TlsConfig::builder()
        .root_certs(RootCerts::PlatformVerifier)
        .build();

    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(15)))
        // Between reads, not total: a model may think for a while, but a dead
        // connection should not hold a thread for ever.
        .timeout_per_read(Some(Duration::from_secs(180)))
        .user_agent(USER_AGENT)
        .tls_config(tls)
        .build()
        .new_agent()
}
