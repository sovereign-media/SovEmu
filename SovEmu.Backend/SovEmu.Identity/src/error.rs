//! What can go wrong, in terms the server maps onto its own API errors.

/// An account or sign-in error. The message is written for people; internal causes are logged,
/// not returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The request itself is wrong (a bad username, a short password).
    BadRequest(String),
    /// Wrong credentials, or not signed in.
    Unauthorized(String),
    /// Signed in, but not allowed.
    Forbidden(String),
    /// Already exists, or already done.
    Conflict(String),
    /// Too many failed attempts: try again later.
    TooManyRequests(String),
    /// The sign-in settings cannot work (an address that is not a URL, none at all).
    Config(String),
    /// Something broke on our side.
    Internal(String),
}

impl Error {
    pub fn message(&self) -> &str {
        match self {
            Self::BadRequest(m)
            | Self::Unauthorized(m)
            | Self::Forbidden(m)
            | Self::Conflict(m)
            | Self::TooManyRequests(m)
            | Self::Config(m)
            | Self::Internal(m) => m,
        }
    }

    pub(crate) fn internal(err: impl std::fmt::Display) -> Self {
        tracing::error!(error = %err, "internal error");
        Self::Internal("internal error".into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for Error {}

impl From<sqlx::Error> for Error {
    fn from(err: sqlx::Error) -> Self {
        Self::internal(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
