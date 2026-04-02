//! This module contains all data structures used throughout the relayer:
//! - Message: Encrypted messages stored and synced to Walrus
//! - Attachment: File attachments linked to messages
//! - GroupMembership: Local cache of group membership for validation

pub mod attachment;
pub mod membership;
pub mod message;

// Re-export commonly used types
pub use attachment::Attachment;
#[allow(unused_imports)]
pub use membership::GroupMembership;
pub use message::{Message, SyncStatus};
