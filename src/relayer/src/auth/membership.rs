//! Group membership storage with permission tracking.
//! Provides a trait for membership storage backends and an in-memory implementation.
//!
//! ## Alignment with Groups SDK Smart Contract
//!
//! The Groups SDK uses permission-based membership:
//! - A member exists if they have at least one permission
//!   - POST message requires `MessagingSender`
//!   - GET message requires `MessagingReader`
//!   - PUT message requires `MessagingEditor`
//!   - DELETE message requires `MessagingDeleter`
//!
//! ## Events from Smart Contract
//!
//! This store should be populated by listening to these Sui events:
//! - `MemberAdded<T>` calls `add_member()` when a new member is added to a group.
//! - `MemberRemoved<T>` calls `remove_member()` when a member is removed from a group.
//! - `PermissionsGranted<T>` calls `grant_permissions()` when a new permission is granted.
//! - `PermissionsRevoked<T>` calls `revoke_permissions()` when a permission is revoked.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use thiserror::Error;
use tracing::debug;

use super::permissions::MessagingPermission;

/// Error type for membership store operations.
#[derive(Debug, Clone, Error)]
pub enum MembershipError {
    #[error("Member {address} not found in group {group_id}")]
    MemberNotFound { group_id: String, address: String },
}

/// Membership store type configuration.
#[derive(Debug, Clone, Default)]
pub enum MembershipStoreType {
    #[default]
    InMemory,
}

/// Trait defining the interface for membership storage backends.
#[allow(dead_code)]
pub trait MembershipStore: Send + Sync {
    /// Checks if an address has a specific permission in a group.
    fn has_permission(
        &self,
        group_id: &str,
        address: &str,
        permission: MessagingPermission,
    ) -> bool;

    /// Checks if an address is a member of a group (has any permission).
    fn is_member(&self, group_id: &str, address: &str) -> bool;

    /// Gets all permissions for a member in a group.
    fn get_permissions(&self, group_id: &str, address: &str) -> Vec<MessagingPermission>;

    /// Grants permissions to a member.
    /// Returns an error if the member does not exist (possible missed MemberAdded event).
    fn grant_permissions(
        &self,
        group_id: &str,
        address: &str,
        permissions: Vec<MessagingPermission>,
    ) -> Result<(), MembershipError>;

    /// Revokes permissions from a member.
    /// Returns an error if the member does not exist.
    fn revoke_permissions(
        &self,
        group_id: &str,
        address: &str,
        permissions: Vec<MessagingPermission>,
    ) -> Result<(), MembershipError>;

    /// Adds a member with initial permissions.
    fn add_member(
        &self,
        group_id: &str,
        address: &str,
        initial_permissions: Vec<MessagingPermission>,
    );

    /// Removes a member completely from a group.
    fn remove_member(&self, group_id: &str, address: &str);

    /// Sets all members and their permissions for a group.
    fn set_group_members(
        &self,
        group_id: &str,
        members_with_perms: Vec<(String, Vec<MessagingPermission>)>,
    );
}

/// Creates a membership store based on the configured store type.
pub fn create_membership_store(store_type: MembershipStoreType) -> Arc<dyn MembershipStore> {
    match store_type {
        MembershipStoreType::InMemory => Arc::new(InMemoryMembershipStore::new()),
    }
}

/// Thread-safe in-memory implementation of MembershipStore.
pub struct InMemoryMembershipStore {
    members: RwLock<HashMap<String, HashMap<String, HashSet<MessagingPermission>>>>,
}

impl InMemoryMembershipStore {
    pub fn new() -> Self {
        Self {
            members: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryMembershipStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MembershipStore for InMemoryMembershipStore {
    fn has_permission(
        &self,
        group_id: &str,
        address: &str,
        permission: MessagingPermission,
    ) -> bool {
        let members = self.members.read().unwrap();

        let result = members
            .get(group_id)
            .and_then(|group_members| group_members.get(address))
            .map(|perms| perms.contains(&permission))
            .unwrap_or(false);

        debug!(
            "has_permission check: group={}, address={}, permission={:?}, result={}",
            group_id, address, permission, result
        );

        result
    }

    fn is_member(&self, group_id: &str, address: &str) -> bool {
        let members = self.members.read().unwrap();

        let result = members
            .get(group_id)
            .and_then(|group_members| group_members.get(address))
            .map(|perms| !perms.is_empty())
            .unwrap_or(false);

        // Log all known groups and members for debugging
        if !result {
            let known_groups: Vec<_> = members.keys().collect();
            debug!(
                "is_member check FAILED: group={}, address={}, known_groups={:?}",
                group_id, address, known_groups
            );
        }

        result
    }

    fn get_permissions(&self, group_id: &str, address: &str) -> Vec<MessagingPermission> {
        let members = self.members.read().unwrap();
        members
            .get(group_id)
            .and_then(|group_members| group_members.get(address))
            .map(|perms| perms.iter().copied().collect())
            .unwrap_or_default()
    }

    fn grant_permissions(
        &self,
        group_id: &str,
        address: &str,
        permissions: Vec<MessagingPermission>,
    ) -> Result<(), MembershipError> {
        let mut members = self.members.write().unwrap();

        // Get the group, return error if group doesn't exist
        let group_members =
            members
                .get_mut(group_id)
                .ok_or_else(|| MembershipError::MemberNotFound {
                    group_id: group_id.to_string(),
                    address: address.to_string(),
                })?;

        // Get the member, return error if member doesn't exist
        let member_perms =
            group_members
                .get_mut(address)
                .ok_or_else(|| MembershipError::MemberNotFound {
                    group_id: group_id.to_string(),
                    address: address.to_string(),
                })?;

        for perm in permissions {
            member_perms.insert(perm);
        }

        Ok(())
    }

    fn revoke_permissions(
        &self,
        group_id: &str,
        address: &str,
        permissions: Vec<MessagingPermission>,
    ) -> Result<(), MembershipError> {
        let mut members = self.members.write().unwrap();

        // Get the group, return error if group doesn't exist
        let group_members =
            members
                .get_mut(group_id)
                .ok_or_else(|| MembershipError::MemberNotFound {
                    group_id: group_id.to_string(),
                    address: address.to_string(),
                })?;

        // Get the member, return error if member doesn't exist
        let member_perms =
            group_members
                .get_mut(address)
                .ok_or_else(|| MembershipError::MemberNotFound {
                    group_id: group_id.to_string(),
                    address: address.to_string(),
                })?;

        for perm in permissions {
            member_perms.remove(&perm);
        }

        Ok(())
    }

    fn add_member(
        &self,
        group_id: &str,
        address: &str,
        initial_permissions: Vec<MessagingPermission>,
    ) {
        let mut members = self.members.write().unwrap();

        let group_members = members.entry(group_id.to_string()).or_default();

        let perm_set: HashSet<MessagingPermission> = initial_permissions.into_iter().collect();
        group_members.insert(address.to_string(), perm_set);
    }

    fn remove_member(&self, group_id: &str, address: &str) {
        let mut members = self.members.write().unwrap();
        if let Some(group_members) = members.get_mut(group_id) {
            group_members.remove(address);
        }
    }

    fn set_group_members(
        &self,
        group_id: &str,
        members_with_perms: Vec<(String, Vec<MessagingPermission>)>,
    ) {
        let mut members = self.members.write().unwrap();
        let mut group_members: HashMap<String, HashSet<MessagingPermission>> = HashMap::new();
        for (address, permissions) in members_with_perms {
            let perm_set: HashSet<MessagingPermission> = permissions.into_iter().collect();
            group_members.insert(address, perm_set);
        }
        members.insert(group_id.to_string(), group_members);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grant_and_check_permission() {
        let store = InMemoryMembershipStore::new();
        let group_id = "group-123";
        let address = "0xaabbccddeeff1122";

        assert!(!store.has_permission(group_id, address, MessagingPermission::MessagingSender));

        // First add the member, then grant permissions
        store.add_member(group_id, address, vec![]);
        store
            .grant_permissions(
                group_id,
                address,
                vec![MessagingPermission::MessagingSender],
            )
            .unwrap();

        assert!(store.has_permission(group_id, address, MessagingPermission::MessagingSender));
        assert!(!store.has_permission(group_id, address, MessagingPermission::MessagingEditor));
    }

    #[test]
    fn test_grant_permissions_fails_if_member_not_found() {
        let store = InMemoryMembershipStore::new();
        let group_id = "group-123";
        let address = "0xaabbccddeeff1122";

        // Granting permissions without adding member should fail
        let result = store.grant_permissions(
            group_id,
            address,
            vec![MessagingPermission::MessagingSender],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_is_member() {
        let store = InMemoryMembershipStore::new();
        let group_id = "group-123";
        let address = "0xaabbccddeeff1122";

        assert!(!store.is_member(group_id, address));

        // Add member with initial permissions
        store.add_member(
            group_id,
            address,
            vec![MessagingPermission::MessagingReader],
        );

        assert!(store.is_member(group_id, address));
    }

    #[test]
    fn test_revoke_permissions() {
        let store = InMemoryMembershipStore::new();
        let group_id = "group-123";
        let address = "0xaabbccddeeff1122";

        // First add the member with permissions
        store.add_member(
            group_id,
            address,
            vec![
                MessagingPermission::MessagingSender,
                MessagingPermission::MessagingEditor,
            ],
        );

        store
            .revoke_permissions(
                group_id,
                address,
                vec![MessagingPermission::MessagingSender],
            )
            .unwrap();

        assert!(!store.has_permission(group_id, address, MessagingPermission::MessagingSender));
        assert!(store.has_permission(group_id, address, MessagingPermission::MessagingEditor));
    }

    #[test]
    fn test_remove_member() {
        let store = InMemoryMembershipStore::new();
        let group_id = "group-123";
        let address = "0xaabbccddeeff1122";

        store.add_member(
            group_id,
            address,
            vec![MessagingPermission::MessagingSender],
        );
        assert!(store.is_member(group_id, address));

        store.remove_member(group_id, address);
        assert!(!store.is_member(group_id, address));
    }

    #[test]
    fn test_get_permissions() {
        let store = InMemoryMembershipStore::new();
        let group_id = "group-123";
        let address = "0xabc123";

        assert!(store.get_permissions(group_id, address).is_empty());

        store.add_member(
            group_id,
            address,
            vec![
                MessagingPermission::MessagingSender,
                MessagingPermission::MessagingReader,
            ],
        );

        let perms = store.get_permissions(group_id, address);
        assert_eq!(perms.len(), 2);
        assert!(perms.contains(&MessagingPermission::MessagingSender));
        assert!(perms.contains(&MessagingPermission::MessagingReader));
    }

    #[test]
    fn test_set_group_members() {
        let store = InMemoryMembershipStore::new();
        let group_id = "group-123";

        store.set_group_members(
            group_id,
            vec![
                (
                    "0xuser1".to_string(),
                    vec![MessagingPermission::MessagingSender],
                ),
                (
                    "0xuser2".to_string(),
                    vec![
                        MessagingPermission::MessagingSender,
                        MessagingPermission::MessagingEditor,
                    ],
                ),
            ],
        );

        assert!(store.has_permission(group_id, "0xuser1", MessagingPermission::MessagingSender));
        assert!(!store.has_permission(group_id, "0xuser1", MessagingPermission::MessagingEditor));

        assert!(store.has_permission(group_id, "0xuser2", MessagingPermission::MessagingSender));
        assert!(store.has_permission(group_id, "0xuser2", MessagingPermission::MessagingEditor));
    }

    #[test]
    fn test_factory_creates_in_memory_store() {
        let store = create_membership_store(MembershipStoreType::InMemory);
        let group_id = "test-group";
        let address = "0xtest";

        store.add_member(group_id, address, vec![]);
        store
            .grant_permissions(
                group_id,
                address,
                vec![MessagingPermission::MessagingSender],
            )
            .unwrap();
        assert!(store.has_permission(group_id, address, MessagingPermission::MessagingSender));
    }
}
