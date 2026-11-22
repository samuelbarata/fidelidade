use std::fmt;

/// Convenience alias used throughout the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong while talking to the Fidelidade customer area.
///
/// Wrong credentials are a distinct variant on purpose: insurers lock accounts
/// after a few failed logins, so callers must **never** blindly retry on
/// [`Error::InvalidCredentials`]. Use [`Error::is_auth_failure`] to tell it apart
/// from transient errors.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// NIF/password rejected by the login page. The message is the one it showed.
    #[error("invalid credentials: {0}")]
    InvalidCredentials(String),

    /// The login needs something this crate cannot do (a verification code,
    /// accepting new terms, confirming contact details...). Do it once in a
    /// browser, then try again.
    #[error("the portal requires interactive action in a browser: {0}")]
    InteractionRequired(String),

    /// The session is gone (expired or never established). Log in again.
    #[error("the Fidelidade session has expired")]
    SessionExpired,

    /// The portal answered with something this crate does not understand. Most
    /// likely the website changed.
    #[error("unexpected response while {context}: {detail}")]
    UnexpectedResponse {
        context: &'static str,
        detail: String,
    },

    /// Bad input provided by the caller.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Transport error.
    #[error(transparent)]
    Http(#[from] reqwest::Error),

    /// JSON (de)serialization error.
    #[error(transparent)]
    Json(#[from] serde_json::Error),

    /// Failed to (de)serialize the cookie jar.
    #[error("cookie jar error: {0}")]
    Cookies(String),
}

impl Error {
    /// `true` for errors that mean "the user's credentials were rejected".
    /// Retrying these automatically can get the account locked.
    pub fn is_auth_failure(&self) -> bool {
        matches!(self, Error::InvalidCredentials(_))
    }

    /// `true` when a fresh login is needed.
    pub fn needs_login(&self) -> bool {
        matches!(self, Error::SessionExpired)
    }

    pub(crate) fn unexpected(context: &'static str, detail: impl fmt::Display) -> Self {
        Error::UnexpectedResponse {
            context,
            detail: detail.to_string(),
        }
    }
}
