//! Background services for the relayer.
//! This module contains services that run alongside the HTTP server:
//! - `event_parser`: Parses Sui blockchain events into domain types
//! - `membership_sync`: Subscribes to Sui checkpoints and syncs membership cache
//! - `walrus_sync`: Periodically uploads pending messages to Walrus storage

pub mod event_parser;
pub mod membership_sync;
pub mod walrus_sync;

pub use membership_sync::MembershipSyncService;
pub use walrus_sync::WalrusSyncService;
