//! HTTP handler functions for message CRUD operations.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use uuid::Uuid;

use crate::auth::signature::verify_signature;
use crate::auth::AuthContext;
use crate::models::{Attachment, Message};
use crate::state::AppState;

use super::error::ApiError;
use super::request::{
    AttachmentRequest, CreateMessageRequest, GetMessagesQuery, UpdateMessageRequest,
};
use crate::enclave::{to_signed_response, IntentScope, ProcessedDataResponse, IntentMessage};

use super::response::{
    CreateMessageResponse, EmptyResponse, GetMessagesResponse, MessageResponse,
    MessagesListResponse,
};

/// Default number of messages per page
const DEFAULT_PAGE_LIMIT: usize = 50;
/// Maximum allowed messages per page
const MAX_PAGE_LIMIT: usize = 100;

/// POST /messages - Create a new message. Returns a signed response proving the enclave handled delivery.
pub async fn create_message(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<CreateMessageRequest>,
) -> Result<(StatusCode, Json<ProcessedDataResponse<IntentMessage<CreateMessageResponse>>>), ApiError> {
    // Decode hex-encoded encrypted text
    let encrypted_msg = hex::decode(&req.encrypted_text)
        .map_err(|e| ApiError::BadRequest(format!("Invalid hex in encrypted_text: {}", e)))?;

    // Decode hex-encoded nonce
    let nonce = hex::decode(&req.nonce)
        .map_err(|e| ApiError::BadRequest(format!("Invalid hex in nonce: {}", e)))?;
    if nonce.len() != 12 {
        return Err(ApiError::BadRequest(format!(
            "Nonce must be exactly 12 bytes, got {}",
            nonce.len()
        )));
    }

    // Verify per-message signature over canonical content
    let signature = verify_message_signature(
        &req.message_signature,
        &req.group_id,
        &req.encrypted_text,
        &req.nonce,
        req.key_version,
        &auth,
    )?;

    let attachments = decode_attachments(req.attachments)?;

    // Build the public key with flag prefix for storage
    let mut public_key_with_flag = vec![auth.scheme.flag()];
    public_key_with_flag.extend_from_slice(&auth.public_key);

    // Create message domain object
    let message = Message::new(
        req.group_id,
        req.sender_address,
        encrypted_msg,
        nonce,
        req.key_version,
        attachments,
        signature,
        public_key_with_flag,
    );

    // Store message
    let created = state.storage.create_message(message).await?;

    // Notify the Walrus sync worker that a new message was created.
    let _ = state.sync_notifier.send(());

    let payload = CreateMessageResponse {
        message_id: created.id,
    };
    let timestamp_ms = chrono::Utc::now().timestamp_millis() as u64;
    let signed = to_signed_response(&state.eph_kp, payload, timestamp_ms, IntentScope::MessageDelivery);

    Ok((StatusCode::CREATED, Json(signed)))
}

/// GET /messages - Get single message or paginated list
/// Only returns messages belonging to the group the caller is authorized for.
pub async fn get_messages(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<GetMessagesQuery>,
) -> Result<Json<GetMessagesResponse>, ApiError> {
    // If message_id is provided, return single message
    if let Some(message_id) = query.message_id {
        let message = state.storage.get_message(message_id).await?;

        // Verify the message belongs to the group the caller is authorized for
        if auth.authorized_group.as_deref() != Some(message.group_id.as_str()) {
            return Err(ApiError::Forbidden(
                "Message does not belong to the authorized group".to_string(),
            ));
        }

        let response: MessageResponse = message.into();
        return Ok(Json(GetMessagesResponse::Single(response)));
    }

    // Otherwise, require group_id for paginated list
    let group_id = query
        .group_id
        .ok_or_else(|| ApiError::BadRequest("Either message_id or group_id is required".into()))?;

    // Verify the requested group matches the caller's authorized group
    if auth.authorized_group.as_deref() != Some(group_id.as_str()) {
        return Err(ApiError::Forbidden(
            "Not authorized for this group".to_string(),
        ));
    }

    let limit = query
        .limit
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .min(MAX_PAGE_LIMIT);

    // Fetch one extra to determine hasNext
    let messages = state
        .storage
        .get_messages_by_group(&group_id, query.after_order, query.before_order, limit + 1)
        .await?;

    let has_next = messages.len() > limit;
    let messages: Vec<MessageResponse> =
        messages.into_iter().take(limit).map(|m| m.into()).collect();

    let response = MessagesListResponse { messages, has_next };
    Ok(Json(GetMessagesResponse::List(response)))
}

/// PUT /messages - Update a message. Returns a signed response proving the enclave handled the update.
/// Only the original message sender can edit their own message.
pub async fn update_message(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<UpdateMessageRequest>,
) -> Result<Json<ProcessedDataResponse<IntentMessage<EmptyResponse>>>, ApiError> {
    // Fetch existing message to verify ownership and group membership
    let existing_message = state.storage.get_message(req.message_id).await?;

    // Verify the message belongs to the group the user is authorized for (middleware-verified)
    if auth.authorized_group.as_deref() != Some(existing_message.group_id.as_str()) {
        return Err(ApiError::Forbidden(
            "Message does not belong to the authorized group".to_string(),
        ));
    }

    // Only the original sender can edit their message (middleware-verified address)
    if existing_message.sender_wallet_addr != auth.sender_address {
        return Err(ApiError::Forbidden(
            "Only the original sender can edit this message".to_string(),
        ));
    }

    // Decode hex-encoded encrypted text
    let encrypted_msg = hex::decode(&req.encrypted_text)
        .map_err(|e| ApiError::BadRequest(format!("Invalid hex in encrypted_text: {}", e)))?;

    // Decode hex-encoded nonce
    let nonce = hex::decode(&req.nonce)
        .map_err(|e| ApiError::BadRequest(format!("Invalid hex in nonce: {}", e)))?;
    if nonce.len() != 12 {
        return Err(ApiError::BadRequest(format!(
            "Nonce must be exactly 12 bytes, got {}",
            nonce.len()
        )));
    }

    // Verify per-message signature over canonical content
    let signature = verify_message_signature(
        &req.message_signature,
        &req.group_id,
        &req.encrypted_text,
        &req.nonce,
        req.key_version,
        &auth,
    )?;

    let attachments = decode_attachments(req.attachments)?;

    let mut public_key_with_flag = vec![auth.scheme.flag()];
    public_key_with_flag.extend_from_slice(&auth.public_key);

    // Update message
    state
        .storage
        .update_message(
            req.message_id,
            encrypted_msg,
            nonce,
            req.key_version,
            attachments,
            signature,
            public_key_with_flag,
        )
        .await?;

    let timestamp_ms = chrono::Utc::now().timestamp_millis() as u64;
    let signed = to_signed_response(&state.eph_kp, EmptyResponse {}, timestamp_ms, IntentScope::MessageDelivery);

    Ok(Json(signed))
}

/// DELETE /messages/:message_id - Soft delete a message
/// Only the original message sender can delete their own message.
pub async fn delete_message(
    State(state): State<AppState>,
    Path(message_id): Path<Uuid>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<EmptyResponse>, ApiError> {
    // Fetch existing message to verify ownership and group membership
    let existing_message = state.storage.get_message(message_id).await?;

    // Verify the message belongs to the group the user is authorized for
    if auth.authorized_group.as_deref() != Some(existing_message.group_id.as_str()) {
        return Err(ApiError::Forbidden(
            "Message does not belong to the authorized group".to_string(),
        ));
    }

    // Only the original sender can delete their message
    if existing_message.sender_wallet_addr != auth.sender_address {
        return Err(ApiError::Forbidden(
            "Only the original sender can delete this message".to_string(),
        ));
    }

    state.storage.delete_message(message_id).await?;
    Ok(Json(EmptyResponse {}))
}

/// Verifies the per-message signature over canonical content:
/// "{group_id}:{encrypted_text}:{nonce}:{key_version}"
fn verify_message_signature(
    signature_hex: &str,
    group_id: &str,
    encrypted_text: &str,
    nonce: &str,
    key_version: i64,
    auth: &AuthContext,
) -> Result<Vec<u8>, ApiError> {
    let signature_bytes = hex::decode(signature_hex)
        .map_err(|e| ApiError::BadRequest(format!("Invalid hex in message_signature: {}", e)))?;
    if signature_bytes.len() != 64 {
        return Err(ApiError::BadRequest(format!(
            "message_signature must be exactly 64 bytes, got {}",
            signature_bytes.len()
        )));
    }

    // Canonical message: "group_id:encrypted_text:nonce:key_version"
    let canonical = format!("{}:{}:{}:{}", group_id, encrypted_text, nonce, key_version);

    verify_signature(
        canonical.as_bytes(),
        &signature_bytes,
        &auth.public_key,
        auth.scheme,
    )
    .map_err(|e| ApiError::BadRequest(format!("Message signature verification failed: {}", e)))?;

    Ok(signature_bytes)
}

/// Decodes a list of attachment request DTOs into domain attachments.
fn decode_attachments(requests: Vec<AttachmentRequest>) -> Result<Vec<Attachment>, ApiError> {
    requests
        .into_iter()
        .map(|r| r.try_into_attachment().map_err(ApiError::BadRequest))
        .collect()
}
