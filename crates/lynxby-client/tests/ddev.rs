//! Smoke tests against a deployment that is actually running.
//!
//! Ignored by default so `cargo test` stays offline and fast. They are the
//! quickest way to tell whether the desktop can talk to ddev at all: TLS
//! (mkcert), routing, and the shapes this client parses.
//!
//! ```sh
//! LYNXBY_TEST_BASE_URL=https://projectsovereign.ddev.site \
//!   cargo test -p lynxby-client --test ddev -- --ignored --nocapture
//! ```

use lynxby_client::api::ApiClient;
use lynxby_client::oauth;
use ureq::tls::{RootCerts, TlsConfig};

fn base_url() -> Option<String> {
    std::env::var("LYNXBY_TEST_BASE_URL").ok()
}

fn get(url: &str) -> (u16, String) {
    // The same TLS roots the app uses, so ddev's mkcert certificate is
    // trusted here too.
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .new_agent();

    let mut response = agent.get(url).call().expect("the deployment answers");

    let status = response.status().as_u16();
    let body = response.body_mut().read_to_string().unwrap_or_default();

    (status, body)
}

#[test]
#[ignore = "needs a running deployment"]
fn the_deployment_advertises_the_flow_this_app_uses() {
    let Some(base) = base_url() else {
        eprintln!("LYNXBY_TEST_BASE_URL is not set; nothing to check.");
        return;
    };

    let (status, body) = get(&format!("{base}/.well-known/oauth-authorization-server"));

    assert_eq!(status, 200, "{body}");

    let metadata: serde_json::Value = serde_json::from_str(&body).expect("json");

    assert_eq!(
        metadata["code_challenge_methods_supported"],
        serde_json::json!(["S256"])
    );
    assert!(metadata["authorization_endpoint"].is_string());
    assert!(metadata["token_endpoint"].is_string());
}

#[test]
#[ignore = "needs a running deployment"]
fn an_unknown_code_is_refused_with_a_readable_error() {
    let Some(base) = base_url() else {
        return;
    };

    let error = oauth::exchange_code(
        &base,
        "not-a-client",
        "not-a-code",
        "not-a-verifier",
        "http://127.0.0.1:47833/callback",
    )
    .unwrap_err();

    assert_eq!(error.code(), Some("invalid_client"));
    assert!(!error.to_string().is_empty());
}

#[test]
#[ignore = "needs a running deployment"]
fn a_bogus_token_is_signed_out_not_confused() {
    let Some(base) = base_url() else {
        return;
    };

    let error = ApiClient::new(base, "lynx_0000000000000000000000000000000000000000")
        .me()
        .unwrap_err();

    assert!(error.is_unauthorized());
    assert_eq!(error.code(), Some("token_invalid"));
}
