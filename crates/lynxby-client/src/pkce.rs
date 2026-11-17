//! PKCE for the authorization-code flow (RFC 7636), S256 only.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use rand::RngCore;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pkce {
    verifier: String,
    challenge: String,
}

impl Pkce {
    /// A fresh verifier for one sign-in attempt.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self::from_verifier(URL_SAFE_NO_PAD.encode(bytes))
    }

    /// Used by tests and by a caller that already holds a verifier.
    pub fn from_verifier(verifier: impl Into<String>) -> Self {
        let verifier = verifier.into();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

        Self {
            verifier,
            challenge,
        }
    }

    pub fn verifier(&self) -> &str {
        &self.verifier
    }

    pub fn challenge(&self) -> &str {
        &self.challenge
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7636, appendix B.
    #[test]
    fn matches_the_rfc_7636_example() {
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");

        assert_eq!(
            pkce.challenge(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn a_generated_verifier_is_url_safe_and_long_enough() {
        let pkce = Pkce::generate();

        assert!((43..=128).contains(&pkce.verifier().len()));
        assert!(pkce
            .verifier()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~".contains(c)));
        assert!(!pkce.challenge().contains('='));
    }

    #[test]
    fn two_attempts_do_not_share_a_verifier() {
        assert_ne!(Pkce::generate().verifier(), Pkce::generate().verifier());
    }
}
