//! Storage adapter trait defining the interface for message storage.
//!
//! # Overview
//!
//! The `StorageAdapter` trait abstracts storage operations, allowing developers
//! to choose their preferred storage backend:
//!
//! - **InMemoryStorage**: For temporary storage of messages and attachments
//! - **PostgresStorage**: For persistent storage of messages and attachments
//!
//! # How It Works
//!
//! 1. **Define the trait**: Common interface for all storage operations (CRUD, sync status)
//! 2. **Implement per backend**: Each backend implements the trait with its own logic (InMemoryStorage, PostgresStorage)
//! 3. **Define the storage adapter**: Developer chooses which implementation to use via configuration
//! 4. **Handlers use trait**: HTTP handlers work with `dyn StorageAdapter`, unaware of the backend implementation
//!
//! # Usage
//!
//! ```ignore
//! use std::sync::Arc;
//! use messaging_relayer::storage::{StorageAdapter, InMemoryStorage};
//!
//! #[tokio::main]
//! async fn main() {
//!     // Option 1: In-memory temporary storage
//!     let storage: Arc<dyn StorageAdapter> = Arc::new(InMemoryStorage::new());
//!
//!     // Option 2: PostgreSQL for persistent storage
//!     // let storage: Arc<dyn StorageAdapter> = Arc::new(
//!     //     PostgresStorage::new("postgres://localhost/messaging").await.unwrap()
//!     // );
//!
//!     // Pass to app state, handlers don't know which backend is used
//!     let app_state = AppState { storage };
//!     
//!     // Build router with shared state (app_state)
//!     let app = Router::new()
//!         .route("/messages", post(create_message).get(get_messages))
//!         .with_state(app_state);
//! }
//!
//! // Handlers use the trait, not concrete types
//! async fn create_message(
//!     State(state): State<AppState>,
//!     Json(req): Json<CreateMessageRequest>,
//! ) -> Result<Json<MessageResponse>, ApiError> {
//!     let message = Message::new(req.channel_id, req.sender, req.encrypted_msg, req.nonce, req.key_version, req.attachments);
//!     let created = state.storage.create_message(message).await?;
//!     Ok(Json(created.into()))
//! }
//! ```

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

use crate::models::{Attachment, Message, SyncStatus};

/// Errors that can occur during storage operations.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Message not found: {0}")]
    NotFound(Uuid),

    #[error("Group not found: {0}")]
    #[allow(dead_code)]
    GroupNotFound(String),

    #[error("Duplicate message ID: {0}")]
    DuplicateId(Uuid),

    #[error("Duplicate nonce: a message with this nonce already exists")]
    DuplicateNonce,

    #[error("Storage operation failed: {0}")]
    OperationFailed(String),
}

/// Result type alias for storage operations.
pub type StorageResult<T> = Result<T, StorageError>;

#[async_trait]
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub trait StorageAdapter: Send + Sync {
    /// Checks status of the storage backend.
    /// - **InMemoryStorage**: Always returns `Ok(())`
    /// - **PostgresStorage**: Executes `SELECT 1` to verify connection
    async fn health_check(&self) -> StorageResult<()>;

    /// Creates a new message and assigns the next order number for the group.
    /// The storage layer is responsible for:
    /// 1. Determining the next order value for the group
    /// 2. Setting `message.order` before storing
    /// Returns the message with the assigned order.
    async fn create_message(&self, message: Message) -> StorageResult<Message>;

    /// Retrieves a message by its unique ID from the group.
    /// Returns `StorageError::NotFound` if the message doesn't exist.
    async fn get_message(&self, id: Uuid) -> StorageResult<Message>;

    /// Retrieves messages for a group with bidirectional pagination.
    /// # Pagination
    /// - `after_order`: Get messages with `order > value` (scroll down to newer messages)
    /// - `before_order`: Get messages with `order < value` (scroll up to older messages)
    /// - If both are `None`, returns the most recent messages
    /// - If both are `Some`, `after_order` takes precedence
    /// Results are always ordered by `order` ascending.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Get first page (newest messages - Initial Load)
    /// let messages = storage.get_messages_by_group("group_123", None, None, 50).await?;
    ///
    /// // Scroll down (get messages after order 100 - Load More)
    /// let newer = storage.get_messages_by_group("group_123", Some(100), None, 50).await?;
    ///
    /// // Scroll up (get messages before order 50 - Load Previous)
    /// let older = storage.get_messages_by_group("group_123", None, Some(50), 50).await?;
    /// ```
    async fn get_messages_by_group(
        &self,
        group_id: &str,
        after_order: Option<i64>,
        before_order: Option<i64>,
        limit: usize,
    ) -> StorageResult<Vec<Message>>;

    /// Updates a message's content, nonce, key version, and attachments.
    /// Sets `sync_status` to `UPDATE_PENDING` and updates `updated_at` timestamp.
    /// Returns the updated message.
    async fn update_message(
        &self,
        id: Uuid,
        encrypted_msg: Vec<u8>,
        nonce: Vec<u8>,
        key_version: i64,
        attachments: Vec<Attachment>,
        signature: Vec<u8>,
        public_key: Vec<u8>,
    ) -> StorageResult<Message>;

    /// Marks a message for deletion (soft delete).
    /// Sets `sync_status` to `DELETE_PENDING` and updates `updated_at` timestamp.
    /// The message remains in storage until the Walrus sync job processes it.
    /// Returns the updated message with `DELETE_PENDING` status.
    async fn delete_message(&self, id: Uuid) -> StorageResult<Message>;

    /// Updates the sync status and optionally the quilt_patch_id.
    /// Used by the Walrus background sync job to track message backup state.
    /// Returns the updated message.
    async fn update_sync_status(
        &self,
        id: Uuid,
        status: SyncStatus,
        quilt_patch_id: Option<String>,
    ) -> StorageResult<Message>;

    /// Retrieves messages with a given sync status.
    /// Used by the Walrus sync job to find messages pending backup.
    ///
    /// # Example
    /// ```ignore
    /// // Find messages waiting to be synced to Walrus
    /// let pending = storage.get_messages_by_sync_status(SyncStatus::SyncPending, 100).await?;
    /// for msg in pending {
    ///     // Upload to Walrus...
    ///     storage.update_sync_status(msg.id, SyncStatus::Synced, Some(quilt_id)).await?;
    /// }
    /// ```
    async fn get_messages_by_sync_status(
        &self,
        status: SyncStatus,
        limit: usize,
    ) -> StorageResult<Vec<Message>>;
}
