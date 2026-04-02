//! Request data transfer objects for the messages API.

use serde::Deserialize;
use uuid::Uuid;

use crate::models::Attachment;

/// Wire-format attachment with hex-encoded binary fields.
/// Mirrors the domain `Attachment` but uses strings for JSON transport.
#[derive(Debug, Deserialize)]
pub struct AttachmentRequest {
    pub storage_id: String,
    /// Hex-encoded 12-byte AES-GCM nonce for the attachment data
    pub nonce: String,
    /// Hex-encoded encrypted metadata (filename, mime type, size, etc.)
    pub encrypted_metadata: String,
    /// Hex-encoded 12-byte AES-GCM nonce for the metadata
    pub metadata_nonce: String,
}

impl AttachmentRequest {
    /// Decodes hex fields into the domain `Attachment`.
    /// Returns a descriptive error string on invalid hex.
    pub fn try_into_attachment(self) -> Result<Attachment, String> {
        let nonce = hex::decode(&self.nonce)
            .map_err(|e| format!("Invalid hex in attachment nonce: {}", e))?;
        if nonce.len() != 12 {
            return Err(format!(
                "Attachment nonce must be exactly 12 bytes, got {}",
                nonce.len()
            ));
        }
        let encrypted_metadata = hex::decode(&self.encrypted_metadata)
            .map_err(|e| format!("Invalid hex in attachment encrypted_metadata: {}", e))?;
        let metadata_nonce = hex::decode(&self.metadata_nonce)
            .map_err(|e| format!("Invalid hex in attachment metadata_nonce: {}", e))?;
        if metadata_nonce.len() != 12 {
            return Err(format!(
                "Attachment metadata_nonce must be exactly 12 bytes, got {}",
                metadata_nonce.len()
            ));
        }

        Ok(Attachment {
            storage_id: self.storage_id,
            nonce,
            encrypted_metadata,
            metadata_nonce,
        })
    }
}

/// Request body for POST /messages
#[derive(Debug, Deserialize)]
pub struct CreateMessageRequest {
    /// Group ID where this message belongs
    pub group_id: String,
    /// Hex-encoded encrypted message content
    pub encrypted_text: String,
    /// Hex-encoded 12-byte AES-GCM nonce
    pub nonce: String,
    /// Encryption key version
    pub key_version: i64,
    /// Sender's Sui wallet address
    pub sender_address: String,
    /// Hex-encoded 64-byte signature over "{group_id}:{encrypted_text}:{nonce}:{key_version}"
    pub message_signature: String,
    /// Attachments for this message (optional)
    #[serde(default)]
    pub attachments: Vec<AttachmentRequest>,
}

/// Request body for PUT /messages
/// sender_address and group_id are required for auth middleware validation.
/// The handler also verifies the sender is the original message owner.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct UpdateMessageRequest {
    /// Message ID to update
    pub message_id: Uuid,
    /// Group ID where this message belongs (required for auth)
    pub group_id: String,
    /// Sender's Sui wallet address (verified via signature, must be original sender)
    pub sender_address: String,
    /// New hex-encoded encrypted message content
    pub encrypted_text: String,
    /// Hex-encoded 12-byte AES-GCM nonce
    pub nonce: String,
    /// Encryption key version
    pub key_version: i64,
    /// Hex-encoded 64-byte signature over "{group_id}:{encrypted_text}:{nonce}:{key_version}"
    pub message_signature: String,
    /// Attachments for this message (optional)
    #[serde(default)]
    pub attachments: Vec<AttachmentRequest>,
}

/// Query parameters for GET /messages
#[derive(Debug, Deserialize)]
pub struct GetMessagesQuery {
    /// Get a single message by ID
    pub message_id: Option<Uuid>,
    /// Get messages for a group (required if message_id is not provided)
    pub group_id: Option<String>,
    /// Pagination: get messages with order > after_order
    pub after_order: Option<i64>,
    /// Pagination: get messages with order < before_order
    pub before_order: Option<i64>,
    /// Maximum number of messages to return (default: 50, max: 100)
    pub limit: Option<usize>,
}
