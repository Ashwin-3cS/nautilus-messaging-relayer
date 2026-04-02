//! Background sync worker that periodically uploads messages to Walrus.
//! It handles three sync workflows:
//! - SyncPending to Synced for new messages uploaded for the first time
//! - UpdatePending to Updated for edited messages re-uploaded as new quilt patches
//! - DeletePending to Deleted for deleted messages uploaded with Deleted status
//!
//! Readers group patches by message_id, take the latest updated_at,
//! and hide messages with Deleted status.
//!
//! Sync is triggered by either:
//! - A fixed time interval (every N seconds, configurable)
//! - A message count threshold (every X new messages, configurable)

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::models::SyncStatus;
use crate::storage::StorageAdapter;
use crate::walrus::types::QuiltPatchMetadata;
use crate::walrus::WalrusClient;

/// Prefix for message patch identifiers in Walrus quilts.
/// Must match MSG_PREFIX in walrus-discovery-indexer/src/constants.ts
const MSG_PREFIX: &str = "msg-";

/// Source tag value to identify patches from this relayer.
/// Must match SOURCE_TAG in walrus-discovery-indexer/src/constants.ts
const SOURCE_TAG: &str = "sui-messaging-relayer";

/// Background service that syncs pending messages to Walrus storage.
pub struct WalrusSyncService {
    /// Storage backend to query for pending messages
    storage: Arc<dyn StorageAdapter>,
    /// Walrus HTTP client for uploading quilts
    walrus_client: Arc<WalrusClient>,
    /// How often to run the sync cycle (seconds)
    sync_interval_secs: u64,
    /// Max messages to batch per sync cycle
    batch_size: usize,
    /// Number of Walrus epochs to store each quilt
    storage_epochs: u32,
    /// Each received () means one new message was created.
    sync_rx: mpsc::UnboundedReceiver<()>,
    /// How many new messages trigger an immediate sync (0 = disabled, interval-only)
    message_threshold: usize,
}

impl WalrusSyncService {
    pub fn new(
        config: &Config,
        storage: Arc<dyn StorageAdapter>,
        walrus_client: Arc<WalrusClient>,
        sync_rx: mpsc::UnboundedReceiver<()>,
    ) -> Self {
        Self {
            storage,
            walrus_client,
            sync_interval_secs: config.walrus_sync_interval_secs,
            batch_size: config.walrus_sync_batch_size.min(666),
            storage_epochs: config.walrus_storage_epochs,
            sync_rx,
            message_threshold: config.walrus_sync_message_threshold,
        }
    }

    /// Runs the sync worker forever.
    /// Whichever fires first triggers a sync and resets both the timer and counter.
    pub async fn run(&mut self) {
        info!(
            "Starting WalrusSyncService (interval={}s, batch_size={}, epochs={}, message_threshold={})",
            self.sync_interval_secs, self.batch_size, self.storage_epochs, self.message_threshold
        );

        let mut ticker = interval(Duration::from_secs(self.sync_interval_secs));
        let mut message_count: usize = 0;

        loop {
            // Wait for either the timer tick or a message notification
            tokio::select! {
                _ = ticker.tick() => {
                    debug!("Walrus sync triggered by timer (message_count was {})", message_count);
                }

                // Message notification from handler
                result = self.sync_rx.recv() => {
                    match result {
                        Some(()) => {
                            message_count += 1;

                            if self.message_threshold == 0 || message_count < self.message_threshold {
                                continue;
                            }

                            debug!(
                                "Walrus sync triggered by message threshold ({}/{})",
                                message_count, self.message_threshold
                            );
                        }
                        None => {
                            warn!("Sync notification channel closed, stopping WalrusSyncService");
                            return;
                        }
                    }
                }
            }

            // Run all three sync workflows in sequence
            if let Err(e) = self.sync_pending_messages().await {
                error!("Walrus sync cycle failed (SyncPending): {}", e);
            }
            if let Err(e) = self.sync_updated_messages().await {
                error!("Walrus sync cycle failed (UpdatePending): {}", e);
            }
            if let Err(e) = self.sync_deleted_messages().await {
                error!("Walrus sync cycle failed (DeletePending): {}", e);
            }

            message_count = 0;

            ticker.reset();
        }
    }

    /// Fetches pending messages, uploads as a quilt, updates status.
    pub async fn sync_pending_messages(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.sync_messages(SyncStatus::SyncPending, SyncStatus::Synced, "pending")
            .await
    }

    /// Fetches UpdatePending messages, uploads as new quilt patches, marks Updated.
    /// The old quilt_patch_id is overwritten, the previous patch still exists on
    /// Walrus but is superseded by the new one.
    pub async fn sync_updated_messages(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.sync_messages(SyncStatus::UpdatePending, SyncStatus::Updated, "updated")
            .await
    }

    /// Fetches DeletePending messages, uploads as new quilt patches with Deleted
    /// status, marks Deleted. Readers will know to hide these messages based on the Deleted status.
    pub async fn sync_deleted_messages(
        &self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.sync_messages(SyncStatus::DeletePending, SyncStatus::Deleted, "deleted")
            .await
    }

    /// Shared sync logic: queries messages by `from_status`, serializes each with
    /// `to_status`, uploads as a quilt, and updates storage with the new patch IDs.
    /// The `label` parameter is used for log messages (e.g. "pending", "updated", "deleted").
    async fn sync_messages(
        &self,
        from_status: SyncStatus,
        to_status: SyncStatus,
        label: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // 1. Query storage for messages matching the source status
        let messages = self
            .storage
            .get_messages_by_sync_status(from_status, self.batch_size)
            .await?;

        if messages.is_empty() {
            debug!("No {} messages to sync", label);
            return Ok(());
        }

        info!("Syncing {} {} messages to Walrus", messages.len(), label);

        // 2. Build patches and per-patch metadata tags.
        // Serialize with the target status so the Walrus copy reflects the final state.
        let mut patches: Vec<(String, Vec<u8>)> = Vec::new();
        let mut metadata: Vec<QuiltPatchMetadata> = Vec::new();

        for msg in &messages {
            let identifier = format!("{}{}", MSG_PREFIX, msg.id);
            let mut msg_for_walrus = msg.clone();
            msg_for_walrus.sync_status = to_status;

            match serde_json::to_vec(&msg_for_walrus) {
                Ok(data) => {
                    // Build tags so the indexer can read metadata from the quilt index
                    let mut tags = HashMap::new();
                    tags.insert("source".into(), SOURCE_TAG.into());
                    tags.insert("group_id".into(), msg.group_id.clone());
                    tags.insert("sender".into(), msg.sender_wallet_addr.clone());
                    tags.insert("sync_status".into(), to_status.to_string());
                    tags.insert(
                        "order".into(),
                        msg.order.map(|o| o.to_string()).unwrap_or_default(),
                    );

                    metadata.push(QuiltPatchMetadata {
                        identifier: identifier.clone(),
                        tags,
                    });
                    patches.push((identifier, data));
                }
                Err(e) => {
                    warn!("Failed to serialize {} message {}: {}", label, msg.id, e);
                }
            }
        }

        if patches.is_empty() {
            debug!("No patches to upload after serialization");
            return Ok(());
        }

        // 3. Upload the batch as a single quilt to Walrus with per-patch tags
        let response = self
            .walrus_client
            .store_quilt(patches, Some(metadata), self.storage_epochs)
            .await?;

        info!(
            "Quilt stored on Walrus ({}). Blob ID: {}, patches: {}",
            label,
            response.quilt_blob_id().unwrap_or("unknown"),
            response.stored_quilt_blobs.len()
        );

        // 4. Update each message's sync_status with its new quilt_patch_id
        for msg in &messages {
            let identifier = format!("{}{}", MSG_PREFIX, msg.id);
            match response.get_patch_id(&identifier) {
                Some(patch_id) => {
                    if let Err(e) = self
                        .storage
                        .update_sync_status(msg.id, to_status, Some(patch_id.to_string()))
                        .await
                    {
                        warn!(
                            "Failed to update sync status for {} message {}: {}",
                            label, msg.id, e
                        );
                    }
                }
                None => {
                    warn!(
                        "No patch ID found in Walrus response for {} message {}",
                        label, msg.id
                    );
                }
            }
        }

        info!("Walrus {} sync cycle completed successfully", label);
        Ok(())
    }
}
