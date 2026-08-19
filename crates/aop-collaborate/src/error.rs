//! One error type for every handler, so a failure reaches the client as JSON
//! rather than as an actix default page.
//!
//! Push conflicts are deliberately not in here. A client that is behind has
//! not failed, it has learned something, and its answer carries a body with
//! the missed changes in it. See [`crate::sync`].

use actix_web::{HttpResponse, ResponseError, http::StatusCode};
use serde_json::json;

#[derive(Debug)]
pub enum SyncError {
    /// No usable bearer token, or the issuer said the token is not active.
    Unauthenticated,
    /// A real subject that is not allowed to touch this project.
    Forbidden,
    NotFound,
    BadRequest(String),
    /// The identity provider could not be reached or answered nonsense. This
    /// is separated from `Internal` because it is the failure a self-hoster
    /// hits first, and it needs to point at the issuer setting.
    Idp(String),
    Internal(String),
}

impl SyncError {
    pub fn internal(cause: impl std::fmt::Display) -> Self {
        Self::Internal(cause.to_string())
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Unauthenticated => "unauthenticated",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::BadRequest(_) => "bad_request",
            Self::Idp(_) => "idp_unavailable",
            Self::Internal(_) => "internal",
        }
    }
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthenticated => f.write_str("not authenticated"),
            Self::Forbidden => f.write_str("not allowed"),
            Self::NotFound => f.write_str("not found"),
            Self::BadRequest(why) => write!(f, "{why}"),
            Self::Idp(why) => write!(f, "identity provider: {why}"),
            Self::Internal(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for SyncError {}

impl From<sea_orm::DbErr> for SyncError {
    fn from(err: sea_orm::DbErr) -> Self {
        Self::Internal(err.to_string())
    }
}

impl ResponseError for SyncError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Idp(_) => StatusCode::BAD_GATEWAY,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        // An internal failure's text is for the log, not for the caller: it
        // carries table names and connection strings.
        let message = match self {
            Self::Internal(why) => {
                log::error!("internal error: {why}");
                "internal error".to_string()
            }
            // The reason an identity provider call failed is operational: an
            // operator reading the log is exactly who needs it, and it names
            // no secret. Returning it to the caller while logging nothing
            // meant the server knew why and told only the one party that
            // could do least about it.
            Self::Idp(why) => {
                log::warn!("identity provider: {why}");
                self.to_string()
            }
            other => other.to_string(),
        };
        HttpResponse::build(self.status_code()).json(json!({
            "error": self.code(),
            "message": message,
        }))
    }
}
