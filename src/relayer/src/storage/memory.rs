//! In-memory storage implementation for in-memory storage of messages and attachments.

use async_trait::async_trait;
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;
use uuid::Uuid;

use crate::models::{Attachment, Message, SyncStatus};

use super::adapter::{StorageAdapter, StorageError, StorageResult};

/// In-memory storage backend using HashMaps protected by RwLock for thread-safety.
/// RwLock allows either many readers OR one writer at a time
/// Thread-safe for concurrent access. Data is lost on restart.
///
/// Lock ordering: always acquire `messages` before `nonces` to prevent deadlocks.
pub struct InMemoryStorage {
    /// All messages indexed by ID
    messages: RwLock<HashMap<Uuid, Message>>,
    /// Tracks the highest order number per group
    group_orders: RwLock<HashMap<String, i64>>,
    /// All message nonces for O(1) duplicate detection
    nonces: RwLock<HashSet<Vec<u8>>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            messages: RwLock::new(HashMap::new()),
            group_orders: RwLock::new(HashMap::new()),
            nonces: RwLock::new(HashSet::new()),
        }
    }

    /// Gets the next order number for a specific group (auto-increment)
    fn next_order(&self, group_id: &str) -> StorageResult<i64> {
        let mut orders = self
            .group_orders
            .write()
            .map_err(|e| StorageError::OperationFailed(format!("Lock poisoned: {}", e)))?;
        let next = orders.get(group_id).map(|o| o + 1).unwrap_or(1); // Look up current max order for this group, add 1, store the new max
        orders.insert(group_id.to_string(), next);
        Ok(next)
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StorageAdapter for InMemoryStorage {
    async fn health_check(&self) -> StorageResult<()> {
        Ok(())
    }

    async fn create_message(&self, mut message: Message) -> StorageResult<Message> {
        // Assign next order for the group
        let order = self.next_order(&message.group_id)?;
        message.set_order(order);

        let mut messages = self
            .messages
            .write()
            .map_err(|e| StorageError::OperationFailed(format!("Lock poisoned: {}", e)))?;

        // Check for duplicate ID
        if messages.contains_key(&message.id) {
            return Err(StorageError::DuplicateId(message.id));
        }

        // O(1) nonce duplicate check via HashSet
        let mut nonces = self
            .nonces
            .write()
            .map_err(|e| StorageError::OperationFailed(format!("Lock poisoned: {}", e)))?;
        if nonces.contains(&message.nonce) {
            return Err(StorageError::DuplicateNonce);
        }

        nonces.insert(message.nonce.clone());
        messages.insert(message.id, message.clone());

        Ok(message)
    }

    async fn get_message(&self, id: Uuid) -> StorageResult<Message> {
        let messages = self
            .messages
            .read()
            .map_err(|e| StorageError::OperationFailed(format!("Lock poisoned: {}", e)))?;
        messages.get(&id).cloned().ok_or(StorageError::NotFound(id))
    }

    async fn get_messages_by_group(
        &self,
        group_id: &str,
        after_order: Option<i64>,
        before_order: Option<i64>,
        limit: usize,
    ) -> StorageResult<Vec<Message>> {
        let messages = self
            .messages
            .read()
            .map_err(|e| StorageError::OperationFailed(format!("Lock poisoned: {}", e)))?;

        let mut filtered: Vec<Message> = messages
            .values()
            .filter(|m| m.group_id == group_id)
            .filter(|m| {
                let order = m.order.unwrap_or(0);
                // Apply both after_order and before_order filters together
                // after_order: exclusive lower bound (order > after_order)
                // before_order: exclusive upper bound (order < before_order)
                match (after_order, before_order) {
                    (Some(after), Some(before)) => order > after && order < before,
                    (Some(after), None) => order > after,
                    (None, Some(before)) => order < before,
                    (None, None) => true,
                }
            })
            .cloned()
            .collect();

        // Sort by order ascending
        filtered.sort_by_key(|m| m.order.unwrap_or(0));

        // Apply limit
        filtered.truncate(limit);

        Ok(filtered)
    }

    async fn update_message(
        &self,
        id: Uuid,
        encrypted_msg: Vec<u8>,
        nonce: Vec<u8>,
        key_version: i64,
        attachments: Vec<Attachment>,
        signature: Vec<u8>,
        public_key: Vec<u8>,
    ) -> StorageResult<Message> {
        let mut messages = self
            .messages
            .write()
            .map_err(|e| StorageError::OperationFailed(format!("Lock poisoned: {}", e)))?;

        let message = messages.get_mut(&id).ok_or(StorageError::NotFound(id))?;

        // Check new nonce isn't already used by another message
        let mut nonces = self
            .nonces
            .write()
            .map_err(|e| StorageError::OperationFailed(format!("Lock poisoned: {}", e)))?;
        if message.nonce != nonce && nonces.contains(&nonce) {
            return Err(StorageError::DuplicateNonce);
        }
        // Swap old nonce for new one in the set
        nonces.remove(&message.nonce);
        nonces.insert(nonce.clone());

        message.update_content(
            encrypted_msg,
            nonce,
            key_version,
            attachments,
            signature,
            public_key,
        );

        Ok(message.clone())
    }

    async fn delete_message(&self, id: Uuid) -> StorageResult<Message> {
        let mut messages = self
            .messages
            .write()
            .map_err(|e| StorageError::OperationFailed(format!("Lock poisoned: {}", e)))?;

        let message = messages.get_mut(&id).ok_or(StorageError::NotFound(id))?;

        message.mark_for_deletion();

        Ok(message.clone())
    }

    async fn update_sync_status(
        &self,
        id: Uuid,
        status: SyncStatus,
        quilt_patch_id: Option<String>,
    ) -> StorageResult<Message> {
        let mut messages = self
            .messages
            .write()
            .map_err(|e| StorageError::OperationFailed(format!("Lock poisoned: {}", e)))?;

        let message = messages.get_mut(&id).ok_or(StorageError::NotFound(id))?;

        message.sync_status = status;
        message.updated_at = Utc::now();
        if let Some(patch_id) = quilt_patch_id {
            message.quilt_patch_id = Some(patch_id);
        }

        Ok(message.clone())
    }
    async fn get_messages_by_sync_status(
        &self,
        status: SyncStatus,
        limit: usize,
    ) -> StorageResult<Vec<Message>> {
        let messages = self
            .messages
            .read()
            .map_err(|e| StorageError::OperationFailed(format!("Lock poisoned: {}", e)))?;

        let filtered: Vec<Message> = messages
            .values()
            .filter(|m| m.sync_status == status)
            .take(limit)
            .cloned()
            .collect();

        Ok(filtered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Message;

    fn unique_nonce(i: u8) -> Vec<u8> {
        let mut nonce = vec![0u8; 12];
        nonce[0] = i;
        nonce
    }

    #[tokio::test]
    async fn test_create_message_assigns_order() {
        let storage = InMemoryStorage::new();
        let msg = Message::new(
            "group_1".to_string(),
            "0xabc".to_string(),
            vec![1, 2, 3],
            unique_nonce(0),
            0,
            vec![],
            vec![0u8; 64],
            vec![0u8; 33],
        );

        let created = storage.create_message(msg).await.unwrap();

        assert_eq!(created.order, Some(1));
        assert_eq!(created.group_id, "group_1");
    }

    #[tokio::test]
    async fn test_create_multiple_messages_increments_order() {
        let storage = InMemoryStorage::new();

        let msg1 = Message::new(
            "group_1".to_string(),
            "0xabc".to_string(),
            vec![1],
            unique_nonce(1),
            0,
            vec![],
            vec![0u8; 64],
            vec![0u8; 33],
        );
        let msg2 = Message::new(
            "group_1".to_string(),
            "0xdef".to_string(),
            vec![2],
            unique_nonce(2),
            0,
            vec![],
            vec![0u8; 64],
            vec![0u8; 33],
        );
        let msg3 = Message::new(
            "group_1".to_string(),
            "0x123".to_string(),
            vec![3],
            unique_nonce(3),
            0,
            vec![],
            vec![0u8; 64],
            vec![0u8; 33],
        );

        let created1 = storage.create_message(msg1).await.unwrap();
        let created2 = storage.create_message(msg2).await.unwrap();
        let created3 = storage.create_message(msg3).await.unwrap();

        assert_eq!(created1.order, Some(1));
        assert_eq!(created2.order, Some(2));
        assert_eq!(created3.order, Some(3));
    }

    #[tokio::test]
    async fn test_create_message_rejects_duplicate_nonce() {
        let storage = InMemoryStorage::new();

        let msg1 = Message::new(
            "group_1".to_string(),
            "0xabc".to_string(),
            vec![1],
            unique_nonce(99),
            0,
            vec![],
            vec![0u8; 64],
            vec![0u8; 33],
        );
        let msg2 = Message::new(
            "group_1".to_string(),
            "0xdef".to_string(),
            vec![2],
            unique_nonce(99), // same nonce
            0,
            vec![],
            vec![0u8; 64],
            vec![0u8; 33],
        );

        storage.create_message(msg1).await.unwrap();
        let result = storage.create_message(msg2).await;

        assert!(matches!(result, Err(StorageError::DuplicateNonce)));
    }

    #[tokio::test]
    async fn test_get_message() {
        let storage = InMemoryStorage::new();
        let msg = Message::new(
            "group_1".to_string(),
            "0xabc".to_string(),
            vec![1],
            unique_nonce(0),
            0,
            vec![],
            vec![0u8; 64],
            vec![0u8; 33],
        );
        let msg_id = msg.id;

        storage.create_message(msg).await.unwrap();
        let fetched = storage.get_message(msg_id).await.unwrap();

        assert_eq!(fetched.id, msg_id);
    }

    #[tokio::test]
    async fn test_get_message_not_found() {
        let storage = InMemoryStorage::new();
        let result = storage.get_message(Uuid::new_v4()).await;

        assert!(matches!(result, Err(StorageError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_get_messages_by_group() {
        let storage = InMemoryStorage::new();

        for i in 0..5 {
            let msg = Message::new(
                "group_1".to_string(),
                format!("0x{}", i),
                vec![i as u8],
                unique_nonce(i as u8),
                0,
                vec![],
                vec![0u8; 64],
                vec![0u8; 33],
            );
            storage.create_message(msg).await.unwrap();
        }

        for i in 0..3 {
            let msg = Message::new(
                "group_2".to_string(),
                format!("0x{}", i),
                vec![i as u8],
                unique_nonce(10 + i as u8),
                0,
                vec![],
                vec![0u8; 64],
                vec![0u8; 33],
            );
            storage.create_message(msg).await.unwrap();
        }

        let group1_messages = storage
            .get_messages_by_group("group_1", None, None, 10)
            .await
            .unwrap();
        let group2_messages = storage
            .get_messages_by_group("group_2", None, None, 10)
            .await
            .unwrap();

        assert_eq!(group1_messages.len(), 5);
        assert_eq!(group2_messages.len(), 3);
    }

    #[tokio::test]
    async fn test_get_messages_by_group_pagination_after() {
        let storage = InMemoryStorage::new();

        for i in 0..5 {
            let msg = Message::new(
                "group_1".to_string(),
                format!("0x{}", i),
                vec![i],
                unique_nonce(i),
                0,
                vec![],
                vec![0u8; 64],
                vec![0u8; 33],
            );
            storage.create_message(msg).await.unwrap();
        }

        // Get messages after order 2
        let messages = storage
            .get_messages_by_group("group_1", Some(2), None, 10)
            .await
            .unwrap();

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].order, Some(3));
        assert_eq!(messages[1].order, Some(4));
        assert_eq!(messages[2].order, Some(5));
    }

    #[tokio::test]
    async fn test_get_messages_by_group_pagination_before() {
        let storage = InMemoryStorage::new();

        for i in 0..5 {
            let msg = Message::new(
                "group_1".to_string(),
                format!("0x{}", i),
                vec![i],
                unique_nonce(i),
                0,
                vec![],
                vec![0u8; 64],
                vec![0u8; 33],
            );
            storage.create_message(msg).await.unwrap();
        }

        // Get messages before order 4
        let messages = storage
            .get_messages_by_group("group_1", None, Some(4), 10)
            .await
            .unwrap();

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].order, Some(1));
        assert_eq!(messages[1].order, Some(2));
        assert_eq!(messages[2].order, Some(3));
    }

    #[tokio::test]
    async fn test_delete_message_sets_status() {
        let storage = InMemoryStorage::new();
        let msg = Message::new(
            "group_1".to_string(),
            "0xabc".to_string(),
            vec![1],
            unique_nonce(0),
            0,
            vec![],
            vec![0u8; 64],
            vec![0u8; 33],
        );
        let msg_id = msg.id;

        storage.create_message(msg).await.unwrap();
        let deleted = storage.delete_message(msg_id).await.unwrap();

        assert_eq!(deleted.sync_status, SyncStatus::DeletePending);
    }

    #[tokio::test]
    async fn test_update_sync_status() {
        let storage = InMemoryStorage::new();
        let msg = Message::new(
            "group_1".to_string(),
            "0xabc".to_string(),
            vec![1],
            unique_nonce(0),
            0,
            vec![],
            vec![0u8; 64],
            vec![0u8; 33],
        );
        let msg_id = msg.id;

        storage.create_message(msg).await.unwrap();
        let updated = storage
            .update_sync_status(msg_id, SyncStatus::Synced, Some("quilt_123".to_string()))
            .await
            .unwrap();

        assert_eq!(updated.sync_status, SyncStatus::Synced);
        assert_eq!(updated.quilt_patch_id, Some("quilt_123".to_string()));
    }

    #[tokio::test]
    async fn test_get_messages_by_sync_status() {
        let storage = InMemoryStorage::new();

        // Create 3 messages (all SYNC_PENDING by default)
        for i in 0..3 {
            let msg = Message::new(
                "group_1".to_string(),
                format!("0x{}", i),
                vec![i],
                unique_nonce(i),
                0,
                vec![],
                vec![0u8; 64],
                vec![0u8; 33],
            );
            storage.create_message(msg).await.unwrap();
        }

        let pending = storage
            .get_messages_by_sync_status(SyncStatus::SyncPending, 10)
            .await
            .unwrap();

        assert_eq!(pending.len(), 3);
    }

    fn sample_attachment(id: &str) -> Attachment {
        Attachment {
            storage_id: format!("patch-{}", id),
            nonce: vec![0xaa; 12],
            encrypted_metadata: vec![0xca, 0xfe],
            metadata_nonce: vec![0xdd; 12],
        }
    }

    #[tokio::test]
    async fn test_create_message_with_attachments() {
        let storage = InMemoryStorage::new();
        let attachments = vec![sample_attachment("1"), sample_attachment("2")];

        let msg = Message::new(
            "group_1".to_string(),
            "0xabc".to_string(),
            vec![1, 2, 3],
            unique_nonce(50),
            0,
            attachments.clone(),
            vec![0u8; 64],
            vec![0u8; 33],
        );
        let msg_id = msg.id;

        storage.create_message(msg).await.unwrap();
        let fetched = storage.get_message(msg_id).await.unwrap();

        assert_eq!(fetched.attachments.len(), 2);
        assert_eq!(fetched.attachments, attachments);
    }

    #[tokio::test]
    async fn test_update_message_replaces_attachments() {
        let storage = InMemoryStorage::new();

        let msg = Message::new(
            "group_1".to_string(),
            "0xabc".to_string(),
            vec![1],
            unique_nonce(60),
            0,
            vec![sample_attachment("original")],
            vec![0u8; 64],
            vec![0u8; 33],
        );
        let msg_id = msg.id;
        storage.create_message(msg).await.unwrap();

        let new_attachments = vec![sample_attachment("a"), sample_attachment("b")];
        let updated = storage
            .update_message(
                msg_id,
                vec![2],
                unique_nonce(61),
                1,
                new_attachments.clone(),
                vec![0u8; 64],
                vec![0u8; 33],
            )
            .await
            .unwrap();

        assert_eq!(updated.attachments, new_attachments);
        assert_eq!(updated.sync_status, SyncStatus::UpdatePending);

        // Verify via separate get
        let fetched = storage.get_message(msg_id).await.unwrap();
        assert_eq!(fetched.attachments, new_attachments);
    }
}
