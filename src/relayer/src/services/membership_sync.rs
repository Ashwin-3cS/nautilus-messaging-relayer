//! A service that subscribes to Sui checkpoints and syncs membership cache.
//!
//! This service:
//! - Connects to a Sui fullnode via gRPC
//! - Subscribes to the checkpoint stream using SubscriptionService
//! - Filters events from the Groups SDK package
//! - Parses events and updates the MembershipCache
//! - Runs in a loop automatically reconnecting on errors

use std::sync::Arc;

use sui_rpc::field::{FieldMask, FieldMaskUtil};
use sui_rpc::proto::sui::rpc::v2::subscription_service_client::SubscriptionServiceClient;
use sui_rpc::proto::sui::rpc::v2::SubscribeCheckpointsRequest;
use tokio_stream::StreamExt;
use tracing::{debug, error, info, warn};

use crate::auth::MembershipStore;
use crate::config::Config;

use super::event_parser::{parse_sui_event, GroupsEvent};

pub struct MembershipSyncService {
    sui_rpc_url: String,
    groups_package_id: String,
    membership_store: Arc<dyn MembershipStore>,
    last_cursor: Option<u64>,
}

impl MembershipSyncService {
    pub fn new(config: &Config, membership_store: Arc<dyn MembershipStore>) -> Self {
        Self {
            sui_rpc_url: config.sui_rpc_url.clone(),
            groups_package_id: config.groups_package_id.clone(),
            membership_store,
            last_cursor: None,
        }
    }

    /// Runs the sync service forever, reconnecting on errors
    pub async fn run(&mut self) {
        info!(
            "Starting MembershipSyncService, connecting to {}",
            self.sui_rpc_url
        );
        info!("Filtering events for package: {}", self.groups_package_id);

        loop {
            match self.run_subscription().await {
                Ok(()) => {
                    warn!("Checkpoint subscription ended unexpectedly, reconnecting...");
                }
                Err(e) => {
                    error!("Subscription error: {}, reconnecting in 5 seconds...", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    /// Connects to the Sui fullnode and processes the checkpoint stream.
    pub async fn run_subscription(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut client = SubscriptionServiceClient::connect(self.sui_rpc_url.clone()).await?;

        info!("Connected to Sui fullnode, subscribing to checkpoints...");

        // Build the subscription request with field mask
        let mut request = SubscribeCheckpointsRequest::default();
        request.read_mask = Some(FieldMask::from_str("transactions.events"));

        let mut stream = client.subscribe_checkpoints(request).await?.into_inner();

        info!("Subscribed to checkpoint stream");

        // Process each checkpoint as it arrives
        while let Some(response) = stream.next().await {
            let checkpoint_response = response?;

            // Get the cursor (checkpoint sequence number)
            let cursor = checkpoint_response.cursor.unwrap_or(0);

            // Skip if we've already processed this checkpoint (shouldn't happen)
            if let Some(last) = self.last_cursor {
                if cursor <= last {
                    continue;
                }
            }

            // Process the checkpoint if present
            if let Some(checkpoint) = checkpoint_response.checkpoint {
                self.process_checkpoint(&checkpoint, cursor);
            }

            // Update cursor position
            self.last_cursor = Some(cursor);
        }

        Ok(())
    }

    /// Processes a single checkpoint,
    fn process_checkpoint(
        &self,
        checkpoint: &sui_rpc::proto::sui::rpc::v2::Checkpoint,
        cursor: u64,
    ) {
        let mut events_processed = 0;

        // Iterate through all transactions in the checkpoint
        for transaction in &checkpoint.transactions {
            let events = match &transaction.events {
                Some(events) => &events.events,
                None => continue,
            };

            // Process each event
            for event in events {
                if let Some(groups_event) = parse_sui_event(event, &self.groups_package_id) {
                    self.apply_event(&groups_event);
                    events_processed += 1;
                }
            }
        }

        if events_processed > 0 || cursor.is_multiple_of(100) {
            debug!(
                "Processed checkpoint {}, {} Groups SDK events",
                cursor, events_processed
            );
        }
    }

    /// Applies a parsed GroupsEvent to the membership store
    fn apply_event(&self, event: &GroupsEvent) {
        match event {
            GroupsEvent::MemberAdded { group_id, member } => {
                info!("MemberAdded: {} -> {}", member, group_id);
                self.membership_store.add_member(group_id, member, vec![]);
            }

            GroupsEvent::MemberRemoved { group_id, member } => {
                info!("MemberRemoved: {} from {}", member, group_id);
                self.membership_store.remove_member(group_id, member);
            }

            GroupsEvent::PermissionsGranted {
                group_id,
                member,
                permissions,
            } => {
                info!(
                    "PermissionsGranted: {} -> {} permissions: {:?}",
                    member, group_id, permissions
                );
                if let Err(e) =
                    self.membership_store
                        .grant_permissions(group_id, member, permissions.clone())
                {
                    warn!(
                        "Failed to grant permissions: {} - possible missed MemberAdded event",
                        e
                    );
                }
            }

            GroupsEvent::PermissionsRevoked {
                group_id,
                member,
                permissions,
            } => {
                info!(
                    "PermissionsRevoked: {} from {} permissions: {:?}",
                    member, group_id, permissions
                );
                if let Err(e) =
                    self.membership_store
                        .revoke_permissions(group_id, member, permissions.clone())
                {
                    warn!(
                        "Failed to revoke permissions: {} - possible missed MemberAdded event",
                        e
                    );
                }
            }
        }
    }
}
