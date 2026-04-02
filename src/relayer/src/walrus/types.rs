//! Walrus API response types and error definitions.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

// Error Types
#[derive(Debug, Error)]
pub enum WalrusError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    #[error("Walrus API error: {status} - {message}")]
    ApiError { status: u16, message: String },

    #[error("Failed to parse response: {0}")]
    ParseError(String),

    #[error("Blob not found: {0}")]
    #[allow(dead_code)]
    NotFound(String),
}

/// Response from storing a single blob via `PUT /v1/blobs?epochs=N`.
/// The publisher returns one of two variants:
/// - `newly_created`: Blob was stored for the first time
/// - `already_certified`: Blob with identical content already exists
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobStoreResponse {
    /// Present if this blob was newly stored
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newly_created: Option<NewlyCreatedBlob>,

    /// Present if this exact blob content already exists on Walrus
    #[serde(skip_serializing_if = "Option::is_none")]
    pub already_certified: Option<AlreadyCertifiedBlob>,
}

impl BlobStoreResponse {
    /// Extracts the BlobId
    pub fn blob_id(&self) -> Option<&str> {
        if let Some(ref newly_created) = self.newly_created {
            Some(&newly_created.blob_object.blob_id)
        } else if let Some(ref already_certified) = self.already_certified {
            Some(&already_certified.blob_id)
        } else {
            None
        }
    }
}

/// Details when a blob is newly created and stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewlyCreatedBlob {
    pub blob_object: BlobObject,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<u64>,
}

/// Details when a blob with identical content already exists.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlreadyCertifiedBlob {
    pub blob_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_epoch: Option<u64>,
}

/// A blob object returned by the publisher.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobObject {
    pub blob_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

/// Response from storing a quilt via `PUT /v1/quilts?epochs=N`.
/// A quilt batches multiple blobs (patches) into a single storage unit.
/// The response includes:
/// - The overall quilt's blob storage result
/// - Individual patch info for each blob in the quilt
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuiltStoreResponse {
    pub blob_store_result: BlobStoreResponse,

    pub stored_quilt_blobs: Vec<StoredQuiltPatch>,
}

impl QuiltStoreResponse {
    /// Returns the overall quilt's BlobId.
    pub fn quilt_blob_id(&self) -> Option<&str> {
        self.blob_store_result.blob_id()
    }

    pub fn get_patch_id(&self, identifier: &str) -> Option<&str> {
        self.stored_quilt_blobs
            .iter()
            .find(|p| p.identifier == identifier)
            .map(|p| p.quilt_patch_id.as_str())
    }
}

/// Information about a single patch stored within a quilt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredQuiltPatch {
    /// The identifier used when uploading this patch.
    /// `msg-{message_id}` format.
    pub identifier: String,

    /// The unique QuiltPatchId for this patch.
    /// This ID can be used to retrieve just this patch from the aggregator
    /// via `GET /v1/blobs/by-quilt-patch-id/{quilt_patch_id}`.
    pub quilt_patch_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<QuiltRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuiltRange {
    pub start: u64,
    pub end: u64,
}

/// Information about a single patch when listing quilt contents.
/// The aggregator's list endpoint returns a flat JSON array of these objects.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchInfo {
    /// The identifier given when uploading this patch
    pub identifier: String,

    /// The unique QuiltPatchId for retrieving this patch individually
    pub patch_id: String,

    /// Optional tags attached to this patch
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<serde_json::Value>,
}

/// Per-patch metadata sent as the `_metadata` form part when storing a quilt.
/// The publisher uses this to embed tags in the quilt index.
#[derive(Debug, Clone, Serialize)]
pub struct QuiltPatchMetadata {
    pub identifier: String,
    pub tags: HashMap<String, String>,
}

pub type WalrusResult<T> = Result<T, WalrusError>;
