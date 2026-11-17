//! The whole sign-in, in the order the person experiences it.
//!
//! `begin` prepares a loopback listener and a client id; the caller opens the
//! browser; `wait` blocks until the code arrives and trades it for a token.
//! Splitting it there keeps the browser launch (a side effect) out of the test
//! path.

use std::sync::atomic::AtomicBool;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::Error;
use crate::oauth::{self, Loopback};
use crate::pkce::Pkce;
use crate::store::{Registration, Session, Store};

pub struct SignIn {
    base_url: String,
    client_id: String,
    redirect_uri: String,
    state: String,
    pkce: Pkce,
    loopback: Loopback,
    pub authorize_url: String,
}

impl SignIn {
    pub fn begin(base_url: &str, port: u16, store: &Store) -> Result<Self, Error> {
        Self::begin_with(base_url, port, store, None)
    }

    /// `client_id` is for a deployment that hands clients out instead of
    /// accepting registrations.
    pub fn begin_with(
        base_url: &str,
        port: u16,
        store: &Store,
        client_id: Option<String>,
    ) -> Result<Self, Error> {
        let loopback = Loopback::bind(port)?;
        let redirect_uri = format!("http://127.0.0.1:{}/callback", loopback.port());

        let client_id = client_id_for(base_url, &redirect_uri, store, client_id)?;

        let pkce = Pkce::generate();
        let state = oauth::new_state();
        let authorize_url =
            oauth::authorize_url(base_url, &client_id, &redirect_uri, &state, &pkce);

        Ok(Self {
            base_url: base_url.to_string(),
            client_id,
            redirect_uri,
            state,
            pkce,
            loopback,
            authorize_url,
        })
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Blocks until the browser comes back, then trades the code for a token.
    pub fn wait(self, cancel: &AtomicBool) -> Result<Session, Error> {
        let code = self.loopback.wait_for_code(&self.state, cancel)?;
        let tokens = oauth::exchange_code(
            &self.base_url,
            &self.client_id,
            &code,
            self.pkce.verifier(),
            &self.redirect_uri,
        )?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default();

        Ok(Session {
            base_url: self.base_url,
            client_id: self.client_id,
            redirect_uri: self.redirect_uri,
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: tokens.expires_in.map(|seconds| now + seconds),
            scope: tokens.scope,
            user: None,
        })
    }
}

/// The client id for this deployment and port: handed out, stored, or newly
/// registered. Split out so the reuse rule is testable without binding a port.
fn client_id_for(
    base_url: &str,
    redirect_uri: &str,
    store: &Store,
    handed_out: Option<String>,
) -> Result<String, Error> {
    if let Some(client_id) = handed_out {
        return Ok(client_id);
    }

    if let Some(registration) = store.load_registration(base_url) {
        if registration.redirect_uri == redirect_uri {
            return Ok(registration.client_id);
        }
    }

    let client_id = oauth::register_client(base_url, redirect_uri)?;

    store.save_registration(&Registration {
        base_url: base_url.to_string(),
        client_id: client_id.clone(),
        redirect_uri: redirect_uri.to_string(),
    })?;

    Ok(client_id)
}

/// The blocking sign-in the UI runs on a background thread.
pub fn sign_in(
    base_url: &str,
    port: u16,
    client_id: Option<String>,
    store: &Store,
    cancel: &AtomicBool,
) -> Result<Session, Error> {
    let pending = SignIn::begin_with(base_url, port, store, client_id)?;

    oauth::announce(&pending.authorize_url);
    oauth::open_in_browser(&pending.authorize_url)?;

    let session = pending.wait(cancel)?;
    store.save(&session)?;

    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn mock_registration() -> (String, Arc<AtomicUsize>) {
        let server = tiny_http::Server::http(("127.0.0.1", 0)).unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();

        std::thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                seen.fetch_add(1, Ordering::SeqCst);

                let response = tiny_http::Response::from_string(r#"{"client_id":"cid"}"#)
                    .with_status_code(201)
                    .with_header(
                        "Content-Type: application/json"
                            .parse::<tiny_http::Header>()
                            .unwrap(),
                    );

                let _ = request.respond(response);
            }
        });

        (format!("http://127.0.0.1:{port}"), count)
    }

    fn temp_store() -> Store {
        let nanos = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        Store::new(std::env::temp_dir().join(format!("lynxby-signin-{nanos}")))
    }

    #[test]
    fn a_registration_for_this_port_is_reused_without_a_call() {
        let (base_url, count) = mock_registration();
        let store = temp_store();

        store
            .save_registration(&Registration {
                base_url: base_url.clone(),
                client_id: "stored".to_string(),
                redirect_uri: "http://127.0.0.1:47833/callback".to_string(),
            })
            .unwrap();

        let client_id =
            client_id_for(&base_url, "http://127.0.0.1:47833/callback", &store, None).unwrap();

        assert_eq!(client_id, "stored");
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_registration_for_another_port_registers_again() {
        let (base_url, count) = mock_registration();
        let store = temp_store();

        store
            .save_registration(&Registration {
                base_url: base_url.clone(),
                client_id: "stored".to_string(),
                redirect_uri: "http://127.0.0.1:47833/callback".to_string(),
            })
            .unwrap();

        let client_id =
            client_id_for(&base_url, "http://127.0.0.1:55000/callback", &store, None).unwrap();

        assert_eq!(client_id, "cid");
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_pre_registered_client_is_used_without_a_call() {
        let (base_url, count) = mock_registration();
        let store = temp_store();

        let pending =
            SignIn::begin_with(&base_url, 0, &store, Some("handed-out".to_string())).unwrap();

        assert_eq!(pending.client_id(), "handed-out");
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn begin_registers_and_listens_on_the_port_it_names() {
        let (base_url, count) = mock_registration();
        let store = temp_store();

        let pending = SignIn::begin(&base_url, 0, &store).unwrap();

        assert!(pending.redirect_uri().starts_with("http://127.0.0.1:"));
        assert!(pending.redirect_uri().ends_with("/callback"));
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_authorize_url_carries_the_redirect_and_the_client() {
        let (base_url, _) = mock_registration();
        let store = temp_store();

        let pending = SignIn::begin(&base_url, 0, &store).unwrap();
        let parsed = url::Url::parse(&pending.authorize_url).unwrap();
        let query: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

        assert_eq!(query["redirect_uri"], pending.redirect_uri());
        assert_eq!(query["client_id"], "cid");
        assert!(query["scope"].contains("assistant.execute"));
    }
}
