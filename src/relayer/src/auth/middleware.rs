//! Authentication middleware for Axum.
//! Intercepts GET/POST/PUT/DELETE requests and verifies:
//! 1. Timestamp is within TTL window (replay protection)
//! 2. Signature is valid (Ed25519, Secp256k1, or Secp256r1)
//! 3. Derived Sui address matches claimed sender_address
//! 4. Sender has required permission for the action
//!
//! ## Auth Headers (all authenticated requests)
//!
//! - `X-Signature`: hex-encoded 64-byte raw signature
//! - `X-Public-Key`: hex-encoded (flag_byte || public_key_bytes)
//!
//! ## Requests with a body (POST, PUT)
//!
//! Auth fields (`group_id`, `sender_address`, `timestamp`) are in the JSON body.
//! The signed message is the raw request body bytes.
//!
//! ## Bodyless requests (GET, DELETE)
//!
//! Auth fields come from additional headers:
//! - `X-Group-Id`, `X-Sender-Address`, `X-Timestamp`

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::{
    permissions::MessagingPermission,
    schemes::SignatureScheme,
    signature::{validate_timestamp, verify_address_matches_pubkey, verify_signature},
    types::{AuthContext, AuthError},
    MembershipStore,
};
use crate::config::Config;

/// Auth fields extracted from the request body (POST/PUT).
/// Signature and public_key come from headers instead.
#[derive(Debug, Deserialize)]
struct BodyAuthFields {
    group_id: String,
    sender_address: String,
    timestamp: i64,
}

#[derive(Serialize)]
struct AuthErrorResponse {
    error: String,
    code: String,
}

#[derive(Clone)]
pub struct AuthState {
    pub membership_store: Arc<dyn MembershipStore>,
    pub config: Config,
}

/// Extracts a header value as a trimmed String.
fn get_header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
}

pub async fn auth_middleware(
    State(state): State<AuthState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let required_permission = match method {
        Method::GET => MessagingPermission::MessagingReader,
        Method::POST => MessagingPermission::MessagingSender,
        Method::PUT => MessagingPermission::MessagingEditor,
        Method::DELETE => MessagingPermission::MessagingDeleter,
        _ => return next.run(request).await,
    };

    // Split request into parts (headers, uri, etc.) and body
    let (parts, body) = request.into_parts();

    // 1. Extract signature from X-Signature header
    let signature_hex = match get_header(&parts.headers, "x-signature") {
        Some(v) => v,
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "Missing X-Signature header",
                "MISSING_SIGNATURE",
            );
        }
    };

    // 2. Extract public key from X-Public-Key header
    let public_key_hex = match get_header(&parts.headers, "x-public-key") {
        Some(v) => v,
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "Missing X-Public-Key header",
                "MISSING_PUBLIC_KEY",
            );
        }
    };

    // 3. Decode public key (first byte is the scheme flag)
    let public_key_with_flag = match hex::decode(&public_key_hex) {
        Ok(bytes) => bytes,
        Err(e) => {
            return auth_error_response(
                StatusCode::UNAUTHORIZED,
                AuthError::InvalidPublicKeyFormat(e.to_string()),
            );
        }
    };

    if public_key_with_flag.is_empty() {
        return auth_error_response(
            StatusCode::UNAUTHORIZED,
            AuthError::InvalidPublicKeyFormat("Empty public key".to_string()),
        );
    }

    // Extract scheme flag and determine signature scheme (Ed25519, Secp256k1, Secp256r1)
    let scheme_flag = public_key_with_flag[0];
    let scheme = match SignatureScheme::from_flag(scheme_flag) {
        Some(s) => s,
        None => {
            return auth_error_response(
                StatusCode::UNAUTHORIZED,
                AuthError::InvalidPublicKeyFormat(format!(
                    "Unknown signature scheme flag: 0x{:02x}",
                    scheme_flag
                )),
            );
        }
    };

    // Public key bytes without the flag prefix
    let public_key_bytes = &public_key_with_flag[1..];

    // Validate public key length matches the scheme
    if public_key_bytes.len() != scheme.public_key_length() {
        return auth_error_response(
            StatusCode::UNAUTHORIZED,
            AuthError::InvalidPublicKeyFormat(format!(
                "Expected {} bytes for {}, got {}",
                scheme.public_key_length(),
                scheme,
                public_key_bytes.len()
            )),
        );
    }

    // 4. Decode signature from hex
    let signature_bytes = match hex::decode(&signature_hex) {
        Ok(bytes) => bytes,
        Err(e) => {
            return auth_error_response(
                StatusCode::UNAUTHORIZED,
                AuthError::InvalidSignatureFormat(e.to_string()),
            );
        }
    };

    // 5. Read request body bytes
    let body_bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Failed to read request body",
                "BODY_READ_ERROR",
            );
        }
    };

    // 6. Extract auth fields and determine what was signed.
    //    - POST/PUT have a body: auth fields come from body, signed message = body bytes
    //    - GET/DELETE have no body: auth fields come from headers, signed message = canonical string
    let (group_id, sender_address, timestamp, message_bytes) = if !body_bytes.is_empty() {
        // POST/PUT: parse auth fields from JSON body
        let body_auth: BodyAuthFields = match serde_json::from_slice(&body_bytes) {
            Ok(fields) => fields,
            Err(e) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    &format!("Invalid request body: {}", e),
                    "INVALID_BODY",
                );
            }
        };

        // The entire body is the signed message (no stripping needed)
        let message = body_bytes.to_vec();
        (
            body_auth.group_id,
            body_auth.sender_address,
            body_auth.timestamp,
            message,
        )
    } else {
        // GET/DELETE: parse auth fields from headers
        let sender_address = match get_header(&parts.headers, "x-sender-address") {
            Some(v) => v,
            None => {
                return error_response(
                    StatusCode::UNAUTHORIZED,
                    "Missing X-Sender-Address header",
                    "MISSING_SENDER_ADDRESS",
                );
            }
        };
        let timestamp_str = match get_header(&parts.headers, "x-timestamp") {
            Some(v) => v,
            None => {
                return error_response(
                    StatusCode::UNAUTHORIZED,
                    "Missing X-Timestamp header",
                    "MISSING_TIMESTAMP",
                );
            }
        };
        let group_id = match get_header(&parts.headers, "x-group-id") {
            Some(v) => v,
            None => {
                return error_response(
                    StatusCode::UNAUTHORIZED,
                    "Missing X-Group-Id header",
                    "MISSING_GROUP_ID",
                );
            }
        };
        let timestamp: i64 = match timestamp_str.parse() {
            Ok(t) => t,
            Err(_) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    "Invalid X-Timestamp header: must be a unix timestamp",
                    "INVALID_TIMESTAMP",
                );
            }
        };

        // Canonical signed message for bodyless requests: "timestamp:sender_address:group_id"
        let canonical = format!("{}:{}:{}", timestamp, sender_address, group_id);
        let message = canonical.into_bytes();
        (group_id, sender_address, timestamp, message)
    };

    // 7. Validate timestamp (replay protection)
    if let Err(e) = validate_timestamp(timestamp, state.config.request_ttl_seconds) {
        return auth_error_response(StatusCode::UNAUTHORIZED, e);
    }

    // 8. Verify the signature against the signed message
    if let Err(e) = verify_signature(&message_bytes, &signature_bytes, public_key_bytes, scheme) {
        tracing::warn!("Signature verification failed: {}", e);
        return auth_error_response(StatusCode::UNAUTHORIZED, e);
    }

    // 9. Verify derived address matches claimed sender_address
    if let Err(e) = verify_address_matches_pubkey(&sender_address, public_key_bytes, scheme) {
        return auth_error_response(StatusCode::UNAUTHORIZED, e);
    }

    // 10. Check permission in membership store
    if !state
        .membership_store
        .has_permission(&group_id, &sender_address, required_permission)
    {
        return auth_error_response(
            StatusCode::FORBIDDEN,
            AuthError::NotGroupMember {
                address: sender_address,
                group_id,
            },
        );
    }

    let auth_context = AuthContext {
        sender_address: sender_address.clone(),
        public_key: public_key_bytes.to_vec(),
        scheme,
        authorized_group: Some(group_id.clone()),
    };

    // Reconstruct request with original body and auth context, then forward to handler
    let mut request = Request::from_parts(parts, Body::from(body_bytes));
    request.extensions_mut().insert(auth_context);
    next.run(request).await
}

/// Creates an error response for auth failures.
fn auth_error_response(status: StatusCode, error: AuthError) -> Response {
    let code = match &error {
        AuthError::RequestExpired { .. } => "REQUEST_EXPIRED",
        AuthError::InvalidSignatureFormat(_) => "INVALID_SIGNATURE_FORMAT",
        AuthError::InvalidPublicKeyFormat(_) => "INVALID_PUBLIC_KEY_FORMAT",
        AuthError::SignatureVerificationFailed(_) => "SIGNATURE_VERIFICATION_FAILED",
        AuthError::AddressMismatch { .. } => "ADDRESS_MISMATCH",
        AuthError::NotGroupMember { .. } => "NOT_GROUP_MEMBER",
        AuthError::MissingSignature => "MISSING_SIGNATURE",
        AuthError::MissingPublicKey => "MISSING_PUBLIC_KEY",
        AuthError::MissingTimestamp => "MISSING_TIMESTAMP",
        AuthError::InvalidTimestamp(_) => "INVALID_TIMESTAMP",
    };

    error_response(status, &error.to_string(), code)
}

fn error_response(status: StatusCode, message: &str, code: &str) -> Response {
    let body = AuthErrorResponse {
        error: message.to_string(),
        code: code.to_string(),
    };

    (status, Json(body)).into_response()
}
