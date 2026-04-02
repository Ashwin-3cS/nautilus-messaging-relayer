//! API error types and HTTP response mapping.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use tracing::error;

use crate::storage::StorageError;

/// API error type that maps to HTTP responses.
#[derive(Debug)]
#[allow(dead_code)]
pub enum ApiError {
    /// Resource not found (404)
    NotFound(String),
    /// Bad request - invalid parameters (400)
    BadRequest(String),
    /// Conflict - duplicate resource (409)
    Conflict(String),
    /// Internal server error (500)
    Internal(String),
    /// Unauthorized - invalid or missing authentication (401)
    Unauthorized(String),
    /// Forbidden - authenticated but not authorized (403)
    Forbidden(String),
}
/// Error response body
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            ApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg),
        };

        let body = Json(ErrorResponse { error: message });
        (status, body).into_response()
    }
}

impl From<StorageError> for ApiError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::NotFound(id) => ApiError::NotFound(format!("Message not found: {}", id)),
            StorageError::GroupNotFound(id) => {
                ApiError::NotFound(format!("Group not found: {}", id))
            }
            StorageError::DuplicateId(id) => {
                ApiError::Conflict(format!("Duplicate message ID: {}", id))
            }
            StorageError::DuplicateNonce => ApiError::Conflict(
                "Duplicate nonce: a message with this nonce already exists".to_string(),
            ),
            StorageError::OperationFailed(msg) => {
                error!("Storage operation failed: {}", msg);
                ApiError::Internal(msg)
            }
        }
    }
}
