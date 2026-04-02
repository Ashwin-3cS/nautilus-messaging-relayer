//! Response data transfer objects for the messages API.

use serde::Serialize;
use uuid::Uuid;

use crate::models::{Attachment, Message, SyncStatus};

/// Wire-format attachment with hex-encoded binary fields.
#[derive(Debug, Serialize)]
pub struct AttachmentResponse {
    pub storage_id: String,
    pub nonce: String,
    pub encrypted_metadata: String,
    pub metadata_nonce: String,
}

impl From<Attachment> for AttachmentResponse {
    fn from(att: Attachment) -> Self {
        Self {
            storage_id: att.storage_id,
            nonce: hex::encode(&att.nonce),
            encrypted_metadata: hex::encode(&att.encrypted_metadata),
            metadata_nonce: hex::encode(&att.metadata_nonce),
        }
    }
}

/// Response for a single message
#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub message_id: Uuid,
    pub group_id: String,
    pub order: i64,
    pub encrypted_text: String,
    pub nonce: String,
    pub key_version: i64,
    pub sender_address: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub attachments: Vec<AttachmentResponse>,
    pub is_edited: bool,
    pub is_deleted: bool,
    pub sync_status: String,
    pub quilt_patch_id: Option<String>,
    /// Hex-encoded 64-byte message signature for independent sender verification
    pub signature: String,
    /// Hex-encoded sender public key (flag byte + key bytes)
    pub public_key: String,
}

impl From<Message> for MessageResponse {
    fn from(msg: Message) -> Self {
        // Message is considered edited if updated_at differs from created_at
        let is_edited = msg.updated_at != msg.created_at;

        // Message is considered deleted if sync_status is DeletePending or Deleted
        let is_deleted = matches!(
            msg.sync_status,
            SyncStatus::DeletePending | SyncStatus::Deleted
        );

        Self {
            message_id: msg.id,
            group_id: msg.group_id,
            order: msg.order.unwrap_or(0),
            encrypted_text: hex::encode(&msg.encrypted_msg),
            nonce: hex::encode(&msg.nonce),
            key_version: msg.key_version,
            sender_address: msg.sender_wallet_addr,
            created_at: msg.created_at.timestamp(),
            updated_at: msg.updated_at.timestamp(),
            attachments: msg.attachments.into_iter().map(Into::into).collect(),
            is_edited,
            is_deleted,
            // Expose Walrus archival state so clients can track sync progress
            sync_status: msg.sync_status.to_string(),
            quilt_patch_id: msg.quilt_patch_id,
            signature: hex::encode(&msg.signature),
            public_key: hex::encode(&msg.public_key),
        }
    }
}

/// Response for POST /messages
#[derive(Debug, Serialize)]
pub struct CreateMessageResponse {
    pub message_id: Uuid,
}

/// Response for GET /messages with pagination
#[derive(Debug, Serialize)]
pub struct MessagesListResponse {
    pub messages: Vec<MessageResponse>,
    #[serde(rename = "hasNext")]
    pub has_next: bool,
}

/// Generic empty response for PUT/DELETE operations
#[derive(Debug, Serialize)]
pub struct EmptyResponse {}

/// Response enum for GET /messages endpoint
/// Uses untagged to serialize as either a single message or a list
#[derive(Debug, Serialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum GetMessagesResponse {
    /// Single message response (when message_id is provided)
    Single(MessageResponse),
    /// Paginated list response (when group_id is provided)
    List(MessagesListResponse),
}
