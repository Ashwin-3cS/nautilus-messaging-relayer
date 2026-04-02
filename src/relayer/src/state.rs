//! Application state shared across all handlers.

use std::sync::Arc;

use nautilus_enclave::EnclaveKeyPair;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::enclave::LogBuffer;
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
    /// Ephemeral Ed25519 keypair generated at enclave boot — used to sign responses.
    pub eph_kp: Arc<EnclaveKeyPair>,
    /// In-memory ring buffer for recent log lines (accessible via GET /logs).
    pub logs: Arc<LogBuffer>,
}

impl AppState {
    pub fn new(
        storage: Arc<dyn StorageAdapter>,
        config: Config,
        sync_notifier: mpsc::UnboundedSender<()>,
        eph_kp: Arc<EnclaveKeyPair>,
        logs: Arc<LogBuffer>,
    ) -> Self {
        Self {
            storage,
            config,
            sync_notifier,
            eph_kp,
            logs,
        }
    }
}
