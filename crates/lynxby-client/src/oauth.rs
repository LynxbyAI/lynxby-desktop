//! The sign-in handshake: authorization code with PKCE, a loopback listener,
//! and the system browser.
//!
//! The password never touches this process. The browser shows the deployment's
//! own sign-in page, with its own second factor and its own consent screen, and
//! the desktop only receives the code the person approved.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rand::RngCore;
use serde_json::json;
use url::Url;

use crate::error::Error;
use crate::http::agent;
use crate::pkce::Pkce;

/// What the desktop asks for: list and talk to assistants, and read the probe
/// that confirms the token. The deployment may narrow this further per person.
pub const SCOPE: &str = "assistant.read assistant.execute knowledge.read";

pub const CLIENT_NAME: &str = "Lynxby Desktop";

/// How long a person has between the browser opening and the code arriving.
pub const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
    pub scope: String,
}

/// A loopback listener that receives exactly one authorization code.
pub struct Loopback {
    server: tiny_http::Server,
    port: u16,
}

impl Loopback {
    pub fn bind(port: u16) -> Result<Self, Error> {
        let server = tiny_http::Server::http(("127.0.0.1", port)).map_err(|error| {
            Error::Local(format!(
                "Cannot listen on 127.0.0.1:{port} for the sign-in callback ({error}). Is another copy running?"
            ))
        })?;

        let bound = server
            .server_addr()
            .to_ip()
            .map(|address| address.port())
            .unwrap_or(port);

        Ok(Self {
            server,
            port: bound,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Blocks until the browser comes back. The state is checked before the
    /// code is believed: it is what proves the answer belongs to this attempt.
    /// `cancel` lets the window give up without waiting out the timeout.
    pub fn wait_for_code(self, expected_state: &str, cancel: &AtomicBool) -> Result<String, Error> {
        let deadline = std::time::Instant::now() + CALLBACK_TIMEOUT;

        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }

            let remaining = deadline.saturating_duration_since(std::time::Instant::now());

            if remaining.is_zero() {
                return Err(Error::Auth(
                    "Sign-in timed out. Start again when you are ready.".to_string(),
                ));
            }

            let request = match self
                .server
                .recv_timeout(remaining.min(Duration::from_secs(1)))
            {
                Ok(Some(request)) => request,
                Ok(None) => continue,
                Err(error) => return Err(Error::Local(error.to_string())),
            };

            let url = format!("http://127.0.0.1:{}{}", self.port, request.url());

            let parsed = match Url::parse(&url) {
                Ok(parsed) => parsed,
                Err(_) => {
                    respond(request, 400, "That request did not carry a usable address.");
                    continue;
                }
            };

            if parsed.path() != "/callback" {
                respond(request, 404, "Nothing here.");
                continue;
            }

            let query: std::collections::HashMap<String, String> =
                parsed.query_pairs().into_owned().collect();

            if let Some(error) = query.get("error") {
                let description = query
                    .get("error_description")
                    .cloned()
                    .unwrap_or_else(|| "The request was declined.".to_string());
                respond(request, 400, &description);

                return Err(Error::Auth(format!("{error}: {description}")));
            }

            if query.get("state") != Some(&expected_state.to_string()) {
                respond(
                    request,
                    400,
                    "This sign-in answer belongs to another attempt.",
                );

                return Err(Error::Auth(
                    "The sign-in answer did not match this attempt. Try again.".to_string(),
                ));
            }

            match query.get("code") {
                Some(code) => {
                    respond(request, 200, "You are signed in. You can close this tab.");
                    return Ok(code.clone());
                }
                None => {
                    respond(request, 400, "The answer carried no code.");
                    return Err(Error::Auth(
                        "The deployment did not send an authorization code.".to_string(),
                    ));
                }
            }
        }
    }
}

fn respond(request: tiny_http::Request, status: u16, message: &str) {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Lynxby</title>\
         <body style=\"font:16px/1.5 system-ui;background:#1d2a31;color:#eef4f7;\
         display:grid;place-items:center;height:100vh;margin:0\">\
         <p style=\"max-width:34rem;text-align:center\">{message}</p>"
    );

    let response = tiny_http::Response::from_string(body)
        .with_status_code(status)
        .with_header(
            "Content-Type: text/html; charset=utf-8"
                .parse::<tiny_http::Header>()
                .expect("static header"),
        );

    let _ = request.respond(response);
}

/// The URL the system browser opens.
pub fn authorize_url(
    base_url: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    pkce: &Pkce,
) -> String {
    let mut url =
        Url::parse(&format!("{base_url}/oauth/authorize")).expect("base url is validated");

    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", SCOPE)
        .append_pair("state", state)
        .append_pair("code_challenge", pkce.challenge())
        .append_pair("code_challenge_method", "S256");

    url.to_string()
}

/// Registers this build as a native public client for one loopback URI.
pub fn register_client(base_url: &str, redirect_uri: &str) -> Result<String, Error> {
    let body = json!({
        "client_name": CLIENT_NAME,
        "application_type": "native",
        "redirect_uris": [redirect_uri],
        "token_endpoint_auth_method": "none",
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
    });

    let mut response = agent()
        .post(&format!("{base_url}/oauth/register"))
        .header("Content-Type", "application/json")
        .send_json(&body)
        .map_err(Error::from)?;

    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().map_err(Error::from)?;

    if status != 201 {
        return Err(decode_error(status, &text));
    }

    let value: serde_json::Value = serde_json::from_str(&text)?;

    value
        .get("client_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::Protocol("The deployment registered no client id.".to_string()))
}

pub fn exchange_code(
    base_url: &str,
    client_id: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenSet, Error> {
    let mut response = agent()
        .post(&format!("{base_url}/oauth/token"))
        .send_form([
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
        ])
        .map_err(Error::from)?;

    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().map_err(Error::from)?;

    if status != 200 {
        return Err(decode_error(status, &text));
    }

    decode_tokens(&text)
}

pub fn refresh(base_url: &str, client_id: &str, refresh_token: &str) -> Result<TokenSet, Error> {
    let mut response = agent()
        .post(&format!("{base_url}/oauth/token"))
        .send_form([
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
        ])
        .map_err(Error::from)?;

    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().map_err(Error::from)?;

    if status != 200 {
        return Err(decode_error(status, &text));
    }

    decode_tokens(&text)
}

fn decode_tokens(text: &str) -> Result<TokenSet, Error> {
    let value: serde_json::Value = serde_json::from_str(text)?;

    let access_token = value
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::Protocol("The answer carried no access token.".to_string()))?
        .to_string();

    Ok(TokenSet {
        access_token,
        refresh_token: value
            .get("refresh_token")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        expires_in: value.get("expires_in").and_then(serde_json::Value::as_i64),
        scope: value
            .get("scope")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn decode_error(status: u16, text: &str) -> Error {
    let value: serde_json::Value = serde_json::from_str(text).unwrap_or(serde_json::Value::Null);

    let code = value
        .get("error")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("oauth_error");

    let message = value
        .get("error_description")
        .or_else(|| value.get("message"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Sign-in failed.");

    Error::http(status, code, message)
}

/// A random value for `state`.
pub fn new_state() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);

    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Opens the deployment's consent page in the person's own browser, where
/// their session, second factor and password manager already live.
pub fn open_in_browser(url: &str) -> Result<(), Error> {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(url);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| {
            Error::Local(format!(
                "Could not open a browser ({error}). Open this address yourself:\n{url}"
            ))
        })
}

/// Prints the address so a headless run can still sign in.
pub fn announce(url: &str) {
    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "Sign in: {url}");
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://projectsovereign.ddev.site";

    #[test]
    fn the_authorize_url_carries_everything_the_server_needs() {
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
        let url = authorize_url(
            BASE,
            "client-1",
            "http://127.0.0.1:47833/callback",
            "state-1",
            &pkce,
        );

        let parsed = Url::parse(&url).unwrap();
        let query: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(parsed.path(), "/oauth/authorize");
        assert_eq!(query["response_type"], "code");
        assert_eq!(query["client_id"], "client-1");
        assert_eq!(query["redirect_uri"], "http://127.0.0.1:47833/callback");
        assert_eq!(query["state"], "state-1");
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(
            query["code_challenge"],
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(query["scope"].contains("assistant.execute"));
    }

    #[test]
    fn the_scope_string_is_space_separated() {
        assert_eq!(
            SCOPE.split(' ').collect::<Vec<_>>(),
            vec!["assistant.read", "assistant.execute", "knowledge.read"]
        );
    }

    #[test]
    fn state_values_are_unique_and_hex() {
        let first = new_state();
        let second = new_state();

        assert_ne!(first, second);
        assert_eq!(first.len(), 32);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn a_token_answer_is_read_whole() {
        let tokens = decode_tokens(
            r#"{"access_token":"a","refresh_token":"r","expires_in":900,"scope":"assistant.read"}"#,
        )
        .unwrap();

        assert_eq!(tokens.access_token, "a");
        assert_eq!(tokens.refresh_token.as_deref(), Some("r"));
        assert_eq!(tokens.expires_in, Some(900));
        assert_eq!(tokens.scope, "assistant.read");
    }

    #[test]
    fn an_answer_without_a_token_is_a_protocol_error() {
        let error = decode_tokens(r#"{"scope":"x"}"#).unwrap_err();

        assert!(error.to_string().contains("no access token"));
    }

    #[test]
    fn the_oauth_error_body_becomes_a_readable_error() {
        let error = decode_error(
            400,
            r#"{"error":"invalid_grant","error_description":"The code expired."}"#,
        );

        assert_eq!(error.code(), Some("invalid_grant"));
        assert!(error.to_string().contains("The code expired."));
    }

    #[test]
    fn an_unparseable_oauth_error_still_says_something() {
        let error = decode_error(500, "<html>oops</html>");

        assert_eq!(error.code(), Some("oauth_error"));
    }

    #[test]
    fn a_loopback_listener_reports_its_port() {
        let loopback = Loopback::bind(0).unwrap();

        assert!(loopback.port() > 0);
    }
}
