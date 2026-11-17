use std::fmt;

/// One error type for the client, because the UI has to render all of them the
/// same way: a sentence a person can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The deployment answered with an API error body.
    Http {
        status: u16,
        code: String,
        message: String,
    },
    /// The deployment could not be reached, or the TLS handshake failed.
    Network(String),
    /// The answer was not what the protocol says it should be.
    Protocol(String),
    /// Sign-in did not complete.
    Auth(String),
    /// The person stopped waiting for the browser.
    Cancelled,
    /// A command line or environment value is wrong.
    Config(String),
    /// A local file or port could not be used.
    Local(String),
}

impl Error {
    pub fn http(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Http {
            status,
            code: code.into(),
            message: message.into(),
        }
    }

    /// The token is gone, revoked or expired: the only fix is signing in again.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::Http { status, .. } if *status == 401)
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }

    /// Signed in, but not allowed: show it, do not sign the person out.
    pub fn is_forbidden(&self) -> bool {
        matches!(self, Self::Http { status, .. } if *status == 403)
    }

    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Http { code, .. } => Some(code),
            _ => None,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Http { status, .. } => Some(*status),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http {
                status,
                code,
                message,
            } => write!(f, "{message} ({code}, HTTP {status})"),
            Self::Network(message) => write!(f, "Cannot reach Lynxby: {message}"),
            Self::Protocol(message) => write!(f, "Unexpected answer: {message}"),
            Self::Auth(message) => write!(f, "{message}"),
            Self::Cancelled => write!(f, "Sign-in cancelled."),
            Self::Config(message) => write!(f, "{message}"),
            Self::Local(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<ureq::Error> for Error {
    fn from(error: ureq::Error) -> Self {
        Self::Network(error.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Protocol(error.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Local(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_unauthorised_and_forbidden() {
        let unauthorized = Error::http(401, "token_expired", "The token has expired.");
        let forbidden = Error::http(403, "forbidden", "You may not do that.");

        assert!(unauthorized.is_unauthorized());
        assert!(!unauthorized.is_forbidden());
        assert!(forbidden.is_forbidden());
        assert!(!forbidden.is_unauthorized());
        assert_eq!(unauthorized.code(), Some("token_expired"));
        assert_eq!(forbidden.status(), Some(403));
    }

    #[test]
    fn a_network_error_is_neither() {
        let error = Error::Network("connection refused".into());

        assert!(!error.is_unauthorized());
        assert!(!error.is_forbidden());
        assert_eq!(error.code(), None);
        assert!(error.to_string().contains("Cannot reach Lynxby"));
    }

    #[test]
    fn displays_the_deployment_message_first() {
        let error = Error::http(422, "validation_failed", "The message is too long.");

        assert_eq!(
            error.to_string(),
            "The message is too long. (validation_failed, HTTP 422)"
        );
    }
}
