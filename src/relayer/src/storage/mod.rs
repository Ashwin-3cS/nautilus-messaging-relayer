//! Storage layer for the messaging relayer.
//!
//! Provides pluggable storage backends via the `StorageAdapter` trait.

use std::sync::Arc;

pub mod adapter;
pub mod memory;
#[allow(unused_imports)]
pub use adapter::{StorageAdapter, StorageError, StorageResult};
pub use memory::InMemoryStorage;

/// Storage backend type configuration
#[derive(Debug, Clone, Default)]
pub enum StorageType {
    #[default]
    InMemory,
    // Postgres(String), // Future: connection URL
}

/// Creates a storage adapter based on the configured storage type.
pub fn create_storage(storage_type: StorageType) -> Arc<dyn StorageAdapter> {
    match storage_type {
        StorageType::InMemory => Arc::new(InMemoryStorage::new()),
    }
}
