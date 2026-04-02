//! Application state shared across all handlers.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::config::Config;
use crate::storage::StorageAdapter;

/// Shared application state passed to all route handlers.
#[derive(Clone)]
pub struct AppState {
    /// Storage backend (in-memory or PostgreSQL)
    pub storage: Arc<dyn StorageAdapter>,
    /// Application configuration (available for handlers that need it)
    #[allow(dead_code)]
    pub config: Config,
    pub sync_notifier: mpsc::UnboundedSender<()>,
}

impl AppState {
    pub fn new(
        storage: Arc<dyn StorageAdapter>,
        config: Config,
        sync_notifier: mpsc::UnboundedSender<()>,
    ) -> Self {
        Self {
            storage,
            config,
            sync_notifier,
        }
    }
}
