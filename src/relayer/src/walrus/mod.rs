//! This module provides an HTTP client for storing and retrieving data
//! from Walrus via public publisher/aggregator endpoints.

pub mod client;
pub mod types;

pub use client::WalrusClient;
#[allow(unused_imports)]
pub use types::{
    BlobStoreResponse, PatchInfo, QuiltStoreResponse, StoredQuiltPatch, WalrusError, WalrusResult,
};
