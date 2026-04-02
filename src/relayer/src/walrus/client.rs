//! Walrus HTTP client for storing and retrieving blobs/quilts.

use std::time::Duration;

use reqwest::multipart::{Form, Part};
use reqwest::Client;
use tracing::{debug, error};

use super::types::{
    BlobStoreResponse, PatchInfo, QuiltPatchMetadata, QuiltStoreResponse, WalrusError, WalrusResult,
};

/// HTTP client for the Walrus publisher and aggregator APIs
#[derive(Debug, Clone)]
pub struct WalrusClient {
    http_client: Client,

    publisher_url: String,

    #[allow(dead_code)]
    aggregator_url: String,
}

impl WalrusClient {
    /// Creates a new WalrusClient with the given publisher and aggregator URLs
    pub fn new(publisher_url: impl Into<String>, aggregator_url: impl Into<String>) -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            http_client,
            publisher_url: publisher_url.into(),
            aggregator_url: aggregator_url.into(),
        }
    }

    /// Stores multiple blobs as a quilt (batch upload).
    ///
    /// # Arguments
    /// * `patches` - Vec of (identifier, data) tuples
    /// * `metadata` - Optional per-patch metadata with tags embedded in the quilt index
    /// * `epochs` - Number of Walrus epochs to store the quilt
    pub async fn store_quilt(
        &self,
        patches: Vec<(impl Into<String>, Vec<u8>)>,
        metadata: Option<Vec<QuiltPatchMetadata>>,
        epochs: u32,
    ) -> WalrusResult<QuiltStoreResponse> {
        if patches.is_empty() {
            return Err(WalrusError::ApiError {
                status: 400,
                message: "Cannot store quilt with zero patches".to_string(),
            });
        }

        let url = format!("{}/v1/quilts?epochs={}", self.publisher_url, epochs);

        debug!(
            "Storing quilt with {} patches for {} epochs",
            patches.len(),
            epochs
        );

        // Build multipart form - each patch becomes a named part
        // The name/identifier is how we'll look up the QuiltPatchId later
        let mut form = Form::new();
        for (identifier, data) in patches {
            let identifier_string = identifier.into();
            let part = Part::bytes(data).file_name(identifier_string.clone());
            form = form.part(identifier_string, part);
        }

        // Add per-patch metadata (tags) as a _metadata JSON form part
        if let Some(meta) = metadata {
            let meta_json =
                serde_json::to_string(&meta).map_err(|e| WalrusError::ParseError(e.to_string()))?;
            form = form.part("_metadata", Part::text(meta_json));
        }

        // Send the request
        let response = self
            .http_client
            .put(&url)
            .multipart(form)
            .send()
            .await
            .map_err(WalrusError::RequestFailed)?;

        // Check status and parse response
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            error!("Walrus store_quilt failed: {} - {}", status.as_u16(), body);
            return Err(WalrusError::ApiError {
                status: status.as_u16(),
                message: body,
            });
        }

        // Parse the JSON response
        let result: QuiltStoreResponse = response
            .json()
            .await
            .map_err(|e| WalrusError::ParseError(e.to_string()))?;

        debug!(
            "Quilt stored successfully. BlobId: {}, patches: {}",
            result.quilt_blob_id().unwrap_or("unknown"),
            result.stored_quilt_blobs.len()
        );

        Ok(result)
    }

    /// Stores a single blob (not batched).
    ///
    /// Use this for larger blobs or when you only have one item.
    /// For multiple small blobs, prefer `store_quilt()`.
    ///
    /// # Arguments
    /// * `data` - Raw bytes to store
    /// * `epochs` - Number of Walrus epochs to store the blob
    ///
    /// # Returns
    /// `BlobStoreResponse` with either:
    /// - `newly_created`: Blob was stored for the first time
    /// - `already_certified`: Identical content already exists (deduplication)
    #[allow(dead_code)]
    pub async fn store_blob(&self, data: Vec<u8>, epochs: u32) -> WalrusResult<BlobStoreResponse> {
        let url = format!("{}/v1/blobs?epochs={}", self.publisher_url, epochs);

        debug!(
            "Storing single blob ({} bytes) for {} epochs",
            data.len(),
            epochs
        );

        // For single blobs, we just PUT the raw bytes
        let response = self
            .http_client
            .put(&url)
            .body(data)
            .send()
            .await
            .map_err(WalrusError::RequestFailed)?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            error!("Walrus store_blob failed: {} - {}", status.as_u16(), body);
            return Err(WalrusError::ApiError {
                status: status.as_u16(),
                message: body,
            });
        }

        let result: BlobStoreResponse = response
            .json()
            .await
            .map_err(|e| WalrusError::ParseError(e.to_string()))?;

        debug!(
            "Blob stored successfully. BlobId: {}",
            result.blob_id().unwrap_or("unknown")
        );

        Ok(result)
    }

    /// Reads a specific patch from a quilt by its QuiltPatchId.
    ///
    /// This is the primary way to retrieve individual messages that were
    /// stored as part of a quilt.
    ///
    /// # Arguments
    /// * `patch_id` - The QuiltPatchId returned when the quilt was stored
    ///
    /// # Returns
    /// The raw bytes of the patch (in our case, a serialized message)
    #[allow(dead_code)]
    pub async fn read_by_patch_id(&self, patch_id: &str) -> WalrusResult<Vec<u8>> {
        let url = format!(
            "{}/v1/blobs/by-quilt-patch-id/{}",
            self.aggregator_url, patch_id
        );

        debug!("Reading patch by ID: {}", patch_id);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(WalrusError::RequestFailed)?;

        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            error!("Walrus read_by_patch_id not found: {}", patch_id);
            return Err(WalrusError::NotFound(patch_id.to_string()));
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            error!(
                "Walrus read_by_patch_id failed: {} - {}",
                status.as_u16(),
                body
            );
            return Err(WalrusError::ApiError {
                status: status.as_u16(),
                message: body,
            });
        }

        let bytes = response.bytes().await.map_err(WalrusError::RequestFailed)?;

        debug!("Read {} bytes from patch {}", bytes.len(), patch_id);

        Ok(bytes.to_vec())
    }

    /// Reads a standalone blob by its BlobId.
    ///
    /// Use this for blobs stored via `store_blob()`, not quilts.
    ///
    /// # Arguments
    /// * `blob_id` - The BlobId returned when the blob was stored
    #[allow(dead_code)]
    pub async fn read_blob(&self, blob_id: &str) -> WalrusResult<Vec<u8>> {
        let url = format!("{}/v1/blobs/{}", self.aggregator_url, blob_id);

        debug!("Reading blob by ID: {}", blob_id);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(WalrusError::RequestFailed)?;

        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            error!("Walrus read_blob not found: {}", blob_id);
            return Err(WalrusError::NotFound(blob_id.to_string()));
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            error!("Walrus read_blob failed: {} - {}", status.as_u16(), body);
            return Err(WalrusError::ApiError {
                status: status.as_u16(),
                message: body,
            });
        }

        let bytes = response.bytes().await.map_err(WalrusError::RequestFailed)?;

        debug!("Read {} bytes from blob {}", bytes.len(), blob_id);

        Ok(bytes.to_vec())
    }

    /// Lists all patches in a quilt.
    ///
    /// enumerate all messages in a quilt
    /// and retrieve them individually.
    ///
    /// # Arguments
    /// * `quilt_blob_id` - The overall quilt's BlobId
    ///
    /// # Returns
    /// List of `PatchInfo` with identifier and QuiltPatchId for each patch
    #[allow(dead_code)]
    pub async fn list_patches(&self, quilt_blob_id: &str) -> WalrusResult<Vec<PatchInfo>> {
        let url = format!(
            "{}/v1/quilts/{}/patches",
            self.aggregator_url, quilt_blob_id
        );

        debug!("Listing patches for quilt: {}", quilt_blob_id);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(WalrusError::RequestFailed)?;

        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            error!("Walrus list_patches not found: {}", quilt_blob_id);
            return Err(WalrusError::NotFound(quilt_blob_id.to_string()));
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            error!("Walrus list_patches failed: {} - {}", status.as_u16(), body);
            return Err(WalrusError::ApiError {
                status: status.as_u16(),
                message: body,
            });
        }

        // The aggregator returns a JSON array of patch objects
        let patches: Vec<PatchInfo> = response
            .json()
            .await
            .map_err(|e| WalrusError::ParseError(e.to_string()))?;

        debug!("Found {} patches in quilt {}", patches.len(), quilt_blob_id);

        Ok(patches)
    }
}
