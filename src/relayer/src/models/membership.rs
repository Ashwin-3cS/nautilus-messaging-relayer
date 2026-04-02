use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents a group membership record in the relayer's local cache.
/// This is a local cache of membership data that will be synchronized via
/// gRPC subscription to Groups SDK events like (MemberAdded, MemberRemoved) TBD.
/// Purpose: Validate that a sender_address is a member of a group_id before accepting messages.
/// NOTE: The Groups SDK is not finalized yet. The exact event schema and
/// membership structure (MemberCap vs membership registry) is TBD.
/// This model should be adjusted once the Groups SDK specification is available.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[allow(dead_code)]
pub struct GroupMembership {
    /// Group ID (from Groups SDK)
    /// Similar to channel_id in messaging context
    pub group_id: String,
    /// Member's Sui wallet address
    pub member_address: String,
    /// Timestamp when this membership record was created locally
    pub created_at: DateTime<Utc>,
    /// Last sync timestamp (updated when we receive events from Groups SDK)
    pub last_synced_at: DateTime<Utc>,
}
#[allow(dead_code)]
impl GroupMembership {
    /// Called when a membership event is received from Groups SDK.
    pub fn new(group_id: String, member_address: String) -> Self {
        let now = Utc::now();
        Self {
            group_id,
            member_address,
            created_at: now,
            last_synced_at: now,
        }
    }

    /// Called when we receive a confirmation event from Groups SDK.
    pub fn touch(&mut self) {
        self.last_synced_at = Utc::now();
    }
}
