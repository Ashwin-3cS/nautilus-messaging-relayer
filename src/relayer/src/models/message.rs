use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

use super::attachment::Attachment;

/// Represents a message in the relayer storage.
/// Messages are received via HTTP POST requests and stored temporarily before
/// being archived to Walrus storage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// Unique message identifier (UUID v4)
    pub id: Uuid,
    /// Group ID where this message belongs
    pub group_id: String,
    /// Message ordering within the group (counter)
    /// None until storage layer assigns it
    pub order: Option<i64>,
    /// Sender's Sui wallet address (verified via signature)
    pub sender_wallet_addr: String,
    /// Encrypted message content (encrypted by client before sending)
    pub encrypted_msg: Vec<u8>,
    /// 12-byte AES-GCM nonce used to encrypt this message
    pub nonce: Vec<u8>,
    /// Encryption key version
    pub key_version: i64,
    /// Message creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
    /// Synchronization status with Walrus storage
    pub sync_status: SyncStatus,
    /// Walrus quilt patch ID after archival (NULL until synced)
    pub quilt_patch_id: Option<String>,
    /// Attachments associated with this message.
    /// Each entry contains the storage ID and encryption metadata needed by
    /// clients to download and decrypt the attachment.
    pub attachments: Vec<Attachment>,
    /// 64-byte cryptographic signature over the message content.
    /// Allows receivers to independently verify the sender authored this message.
    pub signature: Vec<u8>,
    /// Sender's public key (flag byte + key bytes) for signature verification.
    pub public_key: Vec<u8>,
}

/// Tracks the synchronization status of a message with Walrus storage.
/// (SYNC_PENDING | SYNCED | UPDATE_PENDING | UPDATED | DELETE_PENDING | DELETED)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SyncStatus {
    /// Message received by relayer, pending Walrus archival
    SyncPending,
    /// Message successfully archived to Walrus
    Synced,
    /// Message updated, pending re-archival to Walrus
    UpdatePending,
    /// Message updated and re-synced to Walrus
    Updated,
    /// Message marked for deletion, pending removal from Walrus
    DeletePending,
    /// Message deleted from both relayer and Walrus
    Deleted,
}

/// Default to SyncPending
impl Default for SyncStatus {
    fn default() -> Self {
        Self::SyncPending
    }
}

impl fmt::Display for SyncStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SyncStatus::SyncPending => "SYNC_PENDING",
            SyncStatus::Synced => "SYNCED",
            SyncStatus::UpdatePending => "UPDATE_PENDING",
            SyncStatus::Updated => "UPDATED",
            SyncStatus::DeletePending => "DELETE_PENDING",
            SyncStatus::Deleted => "DELETED",
        };
        write!(f, "{}", s)
    }
}
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
impl Message {
    /// Creates a new message from HTTP POST request data.
    /// The message starts in SYNC_PENDING status and has no quilt_patch_id yet.
    pub fn new(
        group_id: String,
        sender_wallet_addr: String,
        encrypted_msg: Vec<u8>,
        nonce: Vec<u8>,
        key_version: i64,
        attachments: Vec<Attachment>,
        signature: Vec<u8>,
        public_key: Vec<u8>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            group_id,
            order: None,
            sender_wallet_addr,
            encrypted_msg,
            nonce,
            key_version,
            created_at: now,
            updated_at: now,
            sync_status: SyncStatus::default(),
            quilt_patch_id: None,
            attachments,
            signature,
            public_key,
        }
    }

    /// Sets the order field (called by storage layer after determining next order)
    pub fn set_order(&mut self, order: i64) {
        self.order = Some(order);
    }

    /// Marks the message as synced to Walrus with the given quilt patchID.
    /// Updates the sync_status to SYNCED and sets the quilt_patch_id
    #[allow(dead_code)]
    pub fn mark_synced(&mut self, quilt_patch_id: String) {
        self.sync_status = SyncStatus::Synced;
        self.quilt_patch_id = Some(quilt_patch_id);
        self.updated_at = Utc::now();
    }

    /// Marks the message for deletion
    /// Sets sync_status to DELETE_PENDING
    pub fn mark_for_deletion(&mut self) {
        self.sync_status = SyncStatus::DeletePending;
        self.updated_at = Utc::now();
    }

    /// Updates the message content and marks it for re-sync
    /// Sets sync_status to UPDATE_PENDING
    pub fn update_content(
        &mut self,
        encrypted_msg: Vec<u8>,
        nonce: Vec<u8>,
        key_version: i64,
        attachments: Vec<Attachment>,
        signature: Vec<u8>,
        public_key: Vec<u8>,
    ) {
        self.encrypted_msg = encrypted_msg;
        self.nonce = nonce;
        self.key_version = key_version;
        self.attachments = attachments;
        self.signature = signature;
        self.public_key = public_key;
        self.sync_status = SyncStatus::UpdatePending;
        self.updated_at = Utc::now();
    }
}
