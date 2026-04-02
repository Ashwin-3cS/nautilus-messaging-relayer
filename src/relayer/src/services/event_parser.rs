//! Parses Sui events from the Groups SDK into domain types.
//! - Filters events by package ID
//! - Parses event type strings to identify PermissionsGranted/Revoked/MemberAdded/Removed
//! - Deserializes BCS-encoded event contents into Rust structs

use crate::auth::MessagingPermission;
use serde::Deserialize;
use sui_rpc::proto::sui::rpc::v2::Event;
use tracing::{debug, warn};

/// Domain events parsed from Groups SDK Sui events
#[derive(Debug, Clone)]
pub enum GroupsEvent {
    MemberAdded {
        group_id: String,
        member: String,
    },
    MemberRemoved {
        group_id: String,
        member: String,
    },
    PermissionsGranted {
        group_id: String,
        member: String,
        permissions: Vec<MessagingPermission>,
    },
    PermissionsRevoked {
        group_id: String,
        member: String,
        permissions: Vec<MessagingPermission>,
    },
}

#[derive(Debug, Deserialize)]
struct MemberEventBcs {
    group_id: [u8; 32],
    member: [u8; 32],
}

#[derive(Debug, Deserialize)]
struct PermissionsEventBcs {
    group_id: [u8; 32],
    member: [u8; 32],
    permissions: Vec<TypeNameBcs>,
}

#[derive(Debug, Deserialize)]
struct TypeNameBcs {
    name: String,
}

/// Parses a Sui event into a GroupsEvent if it's from our Groups SDK package.
pub fn parse_sui_event(event: &Event, groups_package_id: &str) -> Option<GroupsEvent> {
    // Get the event type string ("0x...::permissioned_group::PermissionsGranted<0x...::messaging::Messaging>")
    let event_type = event.event_type.as_ref()?;

    // Check if this event type is from the Groups SDK package
    // This is more reliable than event.package_id which is the calling package
    let normalized_pkg = groups_package_id.trim_start_matches("0x").to_lowercase();
    let event_type_lower = event_type.to_lowercase();

    if !event_type_lower.starts_with(&format!("0x{}", normalized_pkg))
        && !event_type_lower.starts_with(&normalized_pkg)
    {
        return None;
    }

    debug!("Matched event from Groups SDK package: {}", event_type);

    let contents = event.contents.as_ref()?;
    let bcs_bytes = contents.value.as_ref()?;

    let result = if event_type.contains("::PermissionsGranted<") {
        parse_permissions_granted(bcs_bytes)
    } else if event_type.contains("::PermissionsRevoked<") {
        parse_permissions_revoked(bcs_bytes)
    } else if event_type.contains("::MemberAdded<") {
        parse_member_added(bcs_bytes)
    } else if event_type.contains("::MemberRemoved<") {
        parse_member_removed(bcs_bytes)
    } else {
        debug!("Unknown event type from Groups SDK: {}", event_type);
        None
    };

    if let Some(ref evt) = result {
        debug!("Successfully parsed event: {:?}", evt);
    }

    result
}

/// Parses a PermissionsGranted event from BCS bytes
fn parse_permissions_granted(bcs_bytes: &[u8]) -> Option<GroupsEvent> {
    let event_data: PermissionsEventBcs = bcs::from_bytes(bcs_bytes)
        .map_err(|e| warn!("Failed to parse PermissionsGranted BCS: {}", e))
        .ok()?;

    let group_id = format!("0x{}", hex::encode(event_data.group_id));
    let member = format!("0x{}", hex::encode(event_data.member));

    let permissions = event_data
        .permissions
        .iter()
        .filter_map(|tn| MessagingPermission::from_type_name(&tn.name))
        .collect();

    Some(GroupsEvent::PermissionsGranted {
        group_id,
        member,
        permissions,
    })
}

/// Parses a PermissionsRevoked event from BCS bytes
fn parse_permissions_revoked(bcs_bytes: &[u8]) -> Option<GroupsEvent> {
    let event_data: PermissionsEventBcs = bcs::from_bytes(bcs_bytes)
        .map_err(|e| warn!("Failed to parse PermissionsRevoked BCS: {}", e))
        .ok()?;

    let group_id = format!("0x{}", hex::encode(event_data.group_id));
    let member = format!("0x{}", hex::encode(event_data.member));

    let permissions = event_data
        .permissions
        .iter()
        .filter_map(|tn| MessagingPermission::from_type_name(&tn.name))
        .collect();

    Some(GroupsEvent::PermissionsRevoked {
        group_id,
        member,
        permissions,
    })
}

/// Parses a MemberAdded event from BCS bytes
fn parse_member_added(bcs_bytes: &[u8]) -> Option<GroupsEvent> {
    let event_data: MemberEventBcs = bcs::from_bytes(bcs_bytes)
        .map_err(|e| warn!("Failed to parse MemberAdded BCS: {}", e))
        .ok()?;

    let group_id = format!("0x{}", hex::encode(event_data.group_id));
    let member = format!("0x{}", hex::encode(event_data.member));

    Some(GroupsEvent::MemberAdded { group_id, member })
}

/// Parses a MemberRemoved event from BCS bytes
fn parse_member_removed(bcs_bytes: &[u8]) -> Option<GroupsEvent> {
    let event_data: MemberEventBcs = bcs::from_bytes(bcs_bytes)
        .map_err(|e| warn!("Failed to parse MemberRemoved BCS: {}", e))
        .ok()?;

    let group_id = format!("0x{}", hex::encode(event_data.group_id));
    let member = format!("0x{}", hex::encode(event_data.member));

    Some(GroupsEvent::MemberRemoved { group_id, member })
}
