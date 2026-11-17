//! Token persistence between runs.
//!
//! A session is bound to the deployment it was created for: a ddev token must
//! never be sent to production, so a stored session that names another host is
//! ignored rather than reused.
//!
//! shortcut: the file is 0600 in the user's data directory, not the OS
//! keyring; move it behind a keyring when the app ships to customers.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Error;

const FILE_NAME: &str = "session.json";
const REGISTRATION_FILE: &str = "client.json";

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct StoredUser {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub organisation: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Session {
    pub base_url: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Unix seconds, when the deployment said so.
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub user: Option<StoredUser>,
}

impl Session {
    /// Treats a token that expires within a minute as gone, so a request does
    /// not race the clock.
    pub fn is_expired(&self, now: i64) -> bool {
        match self.expires_at {
            Some(expires_at) => expires_at - 60 <= now,
            None => false,
        }
    }

    pub fn refreshable(&self) -> bool {
        self.refresh_token.is_some()
    }
}

/// The client id this build registered for a deployment, kept so a second
/// sign-in does not register a second client.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Registration {
    pub base_url: String,
    pub client_id: String,
    pub redirect_uri: String,
}

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// `$XDG_DATA_HOME/lynxby-desktop` on Linux, the equivalent on macOS and
    /// Windows.
    pub fn default_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("lynxby-desktop")
    }

    pub fn default_store() -> Self {
        Self::new(Self::default_dir())
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(FILE_NAME)
    }

    pub fn registration_path(&self) -> PathBuf {
        self.dir.join(REGISTRATION_FILE)
    }

    pub fn load_registration(&self, base_url: &str) -> Option<Registration> {
        let raw = fs::read_to_string(self.registration_path()).ok()?;
        let registration: Registration = serde_json::from_str(&raw).ok()?;

        (registration.base_url == base_url).then_some(registration)
    }

    pub fn save_registration(&self, registration: &Registration) -> Result<(), Error> {
        fs::create_dir_all(&self.dir)?;
        restrict(&self.dir, 0o700);

        let path = self.registration_path();
        fs::write(&path, serde_json::to_vec_pretty(registration)?)?;
        restrict(&path, 0o600);

        Ok(())
    }

    /// The session for this deployment, if one was stored for it.
    pub fn load_for(&self, base_url: &str) -> Option<Session> {
        let session = self.load()?;

        (session.base_url == base_url).then_some(session)
    }

    pub fn load(&self) -> Option<Session> {
        let raw = fs::read_to_string(self.path()).ok()?;

        serde_json::from_str(&raw).ok()
    }

    pub fn save(&self, session: &Session) -> Result<(), Error> {
        fs::create_dir_all(&self.dir)?;
        restrict(&self.dir, 0o700);

        let path = self.path();
        let body = serde_json::to_vec_pretty(session)?;

        fs::write(&path, body)?;
        restrict(&path, 0o600);

        Ok(())
    }

    pub fn clear(&self) -> Result<(), Error> {
        match fs::remove_file(self.path()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;

    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store() -> Store {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        Store::new(std::env::temp_dir().join(format!("lynxby-store-{nanos}")))
    }

    fn session(base_url: &str) -> Session {
        Session {
            base_url: base_url.to_string(),
            client_id: "client".to_string(),
            redirect_uri: "http://127.0.0.1:47833/callback".to_string(),
            access_token: "lynx_access".to_string(),
            refresh_token: Some("lynx_refresh".to_string()),
            expires_at: Some(1_800_000_000),
            scope: "assistant.read".to_string(),
            user: Some(StoredUser {
                name: "Ada".to_string(),
                organisation: "Lynxby".to_string(),
            }),
        }
    }

    #[test]
    fn a_registration_belongs_to_one_deployment() {
        let store = temp_store();
        store
            .save_registration(&Registration {
                base_url: "https://a.test".to_string(),
                client_id: "cid".to_string(),
                redirect_uri: "http://127.0.0.1:47833/callback".to_string(),
            })
            .unwrap();

        assert_eq!(
            store.load_registration("https://a.test").unwrap().client_id,
            "cid"
        );
        assert!(store.load_registration("https://b.test").is_none());
    }

    #[test]
    fn nothing_stored_means_no_session() {
        assert_eq!(temp_store().load(), None);
    }

    #[test]
    fn a_saved_session_reads_back_whole() {
        let store = temp_store();
        let original = session("https://a.test");

        store.save(&original).unwrap();

        assert_eq!(store.load(), Some(original));
    }

    #[test]
    fn a_session_belongs_to_one_deployment() {
        let store = temp_store();
        store.save(&session("https://a.test")).unwrap();

        assert!(store.load_for("https://a.test").is_some());
        assert!(store.load_for("https://b.test").is_none());
    }

    #[test]
    fn clearing_removes_the_file_and_is_idempotent() {
        let store = temp_store();
        store.save(&session("https://a.test")).unwrap();

        store.clear().unwrap();
        store.clear().unwrap();

        assert_eq!(store.load(), None);
    }

    #[test]
    fn a_corrupt_file_is_treated_as_signed_out() {
        let store = temp_store();
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), b"{ not json").unwrap();

        assert_eq!(store.load(), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let store = temp_store();
        store.save(&session("https://a.test")).unwrap();

        let mode = fs::metadata(store.path()).unwrap().permissions().mode() & 0o777;

        assert_eq!(mode, 0o600);
    }

    #[test]
    fn a_token_is_expired_a_minute_before_it_says_so() {
        let mut session = session("https://a.test");
        session.expires_at = Some(1_000);

        assert!(!session.is_expired(900));
        assert!(session.is_expired(941));
        assert!(session.is_expired(1_000));
    }

    #[test]
    fn a_token_without_an_expiry_is_never_locally_expired() {
        let mut session = session("https://a.test");
        session.expires_at = None;

        assert!(!session.is_expired(i64::MAX / 2));
    }
}
