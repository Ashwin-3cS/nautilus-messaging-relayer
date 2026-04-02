//! Application configuration loaded from environment variables.

use std::env;
use tracing::info;

use crate::auth::MembershipStoreType;
use crate::storage::StorageType;

const DEFAULT_WALRUS_PUBLISHER_URL: &str = "https://publisher.walrus-testnet.walrus.space";
const DEFAULT_WALRUS_AGGREGATOR_URL: &str = "https://aggregator.walrus-testnet.walrus.space";

#[derive(Debug, Clone)]
pub struct Config {
    /// Server port (default: 3000)
    pub port: u16,

    /// default: 900 = 15 minutes
    pub request_ttl_seconds: i64,

    /// Set via STORAGE_TYPE env var: "memory" (default) or "postgres"
    pub storage_type: StorageType,

    /// Set via MEMBERSHIP_STORE_TYPE env var: "memory" (default)
    pub membership_store_type: MembershipStoreType,

    /// Sui fullnode gRPC URL for checkpoint streaming
    pub sui_rpc_url: String,

    /// Groups SDK package ID on Sui
    pub groups_package_id: String,

    /// Walrus Configuration
    /// Walrus publisher URL for storing blobs/quilts.
    /// Default: Walrus testnet public publisher
    #[allow(dead_code)]
    pub walrus_publisher_url: String,

    /// Walrus aggregator URL for reading blobs.
    /// Default: Walrus testnet public aggregator
    #[allow(dead_code)]
    pub walrus_aggregator_url: String,

    /// Number of Walrus epochs to store blobs
    /// Default: 5 epochs
    #[allow(dead_code)]
    pub walrus_storage_epochs: u32,

    /// How often the Walrus sync worker runs (in seconds).
    /// Default: 3600 (1 hour)
    pub walrus_sync_interval_secs: u64,

    /// Max number of messages to batch per sync cycle.
    /// Default: 100, capped at 666 (Walrus quilt size limit)
    pub walrus_sync_batch_size: usize,

    /// Number of new messages that trigger an immediate Walrus sync.
    /// Default: 50, set via WALRUS_SYNC_MESSAGE_THRESHOLD env var.
    /// Set to 0 to disable message-count-based syncing (interval-only).
    pub walrus_sync_message_threshold: usize,
}

impl Config {
    /// Loads configuration from environment variables.
    /// - `PORT`: Server port (default: 3000)
    /// - `REQUEST_TTL_SECONDS`: Request TTL for replay protection (default: 900)
    /// - `STORAGE_TYPE`: Storage backend type (default: "memory")
    /// - `MEMBERSHIP_STORE_TYPE`: Membership store type (default: "memory")
    /// - `SUI_RPC_URL`: Sui fullnode gRPC URL
    /// - `GROUPS_PACKAGE_ID`: Groups SDK package ID
    /// - `WALRUS_PUBLISHER_URL`: Walrus publisher URL (default: testnet)
    /// - `WALRUS_AGGREGATOR_URL`: Walrus aggregator URL (default: testnet)
    /// - `WALRUS_STORAGE_EPOCHS`: How many epochs to store blobs (default: 5)
    pub fn from_env() -> Self {
        let port = env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000);

        let request_ttl_seconds = env::var("REQUEST_TTL_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(900); // 15 minutes default

        // Parse storage type from STORAGE_TYPE env var
        let storage_type = match env::var("STORAGE_TYPE")
            .unwrap_or_else(|_| "memory".to_string())
            .to_lowercase()
            .as_str()
        {
            "memory" => StorageType::InMemory,
            _ => StorageType::InMemory,
        };

        // Parse membership store type from MEMBERSHIP_STORE_TYPE env var
        let membership_store_type = match env::var("MEMBERSHIP_STORE_TYPE")
            .unwrap_or_else(|_| "memory".to_string())
            .to_lowercase()
            .as_str()
        {
            "memory" => MembershipStoreType::InMemory,
            _ => MembershipStoreType::InMemory,
        };

        // Sui event subscription config
        let sui_rpc_url =
            env::var("SUI_RPC_URL").expect("SUI_RPC_URL environment variable is required");
        let groups_package_id = env::var("GROUPS_PACKAGE_ID")
            .expect("GROUPS_PACKAGE_ID environment variable is required");

        // Publisher URL: where we send PUT requests to store blobs
        let walrus_publisher_url = env::var("WALRUS_PUBLISHER_URL")
            .unwrap_or_else(|_| DEFAULT_WALRUS_PUBLISHER_URL.to_string());

        // Aggregator URL: where we send GET requests to read blobs
        let walrus_aggregator_url = env::var("WALRUS_AGGREGATOR_URL")
            .unwrap_or_else(|_| DEFAULT_WALRUS_AGGREGATOR_URL.to_string());

        // Storage epochs: how long blobs persist on Walrus
        // parse::<u32>() converts string to unsigned 32-bit integer
        let walrus_storage_epochs = env::var("WALRUS_STORAGE_EPOCHS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);

        let walrus_sync_interval_secs = env::var("WALRUS_SYNC_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3600);

        let walrus_sync_batch_size = env::var("WALRUS_SYNC_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100)
            .min(666); // Walrus quilt size limit

        // How many new messages trigger an immediate sync (0 = disabled, interval-only)
        let walrus_sync_message_threshold = env::var("WALRUS_SYNC_MESSAGE_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50);

        let config = Self {
            port,
            request_ttl_seconds,
            storage_type,
            membership_store_type,
            sui_rpc_url,
            groups_package_id,
            walrus_publisher_url,
            walrus_aggregator_url,
            walrus_storage_epochs,
            walrus_sync_interval_secs,
            walrus_sync_batch_size,
            walrus_sync_message_threshold,
        };

        info!("Configuration loaded: {:?}", config);
        config
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 3000,
            request_ttl_seconds: 900,
            storage_type: StorageType::InMemory,
            membership_store_type: MembershipStoreType::InMemory,
            sui_rpc_url: String::new(),
            groups_package_id: String::new(),
            walrus_publisher_url: DEFAULT_WALRUS_PUBLISHER_URL.to_string(),
            walrus_aggregator_url: DEFAULT_WALRUS_AGGREGATOR_URL.to_string(),
            walrus_storage_epochs: 5,
            walrus_sync_interval_secs: 3600,
            walrus_sync_batch_size: 100,
            walrus_sync_message_threshold: 50,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.port, 3000);
        assert_eq!(config.request_ttl_seconds, 900);
    }

    #[test]
    fn test_walrus_defaults() {
        let config = Config::default();
        // Verify Walrus defaults point to testnet
        assert!(config.walrus_publisher_url.contains("testnet"));
        assert!(config.walrus_aggregator_url.contains("testnet"));
        assert_eq!(config.walrus_storage_epochs, 5);
    }
}
