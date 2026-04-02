use serde::{Deserialize, Serialize};

/// A message attachment as stored by the relayer.
///
/// The relayer is a passthrough for attachment data — it never uploads, downloads,
/// encrypts, or decrypts attachments. All of that is the SDK client's responsibility.
///
/// The default SDK implementation uses Walrus as the storage backend, in which case
/// `storage_id` is a Walrus quilt-patch-id. Other `StorageAdapter` implementations
/// (e.g. S3, GCS) will use their own identifier scheme — the relayer doesn't
/// interpret this field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Attachment {
    /// Opaque identifier assigned by the client's storage backend.
    /// Used by clients to download the encrypted attachment data.
    ///
    /// - Walrus (default): quilt-patch-id
    /// - S3: object key or pre-signed URL
    /// - Other adapters: whatever ID their `upload()` returns
    pub storage_id: String,
    /// 12-byte AES-GCM nonce used to encrypt the attachment data.
    pub nonce: Vec<u8>,
    /// Encrypted metadata (filename, mime type, size, etc.).
    /// Encrypted client-side before being sent to the relayer.
    pub encrypted_metadata: Vec<u8>,
    /// 12-byte AES-GCM nonce used to encrypt the metadata.
    pub metadata_nonce: Vec<u8>,
}
