//! Authentication data types.

/// Validated authentication context passed to handlers.
///
/// After successful middleware verification, this is attached
/// to request extensions and can be extracted by handlers.
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// Verified sender Sui address (derived from verified public key)
    pub sender_address: String,

    /// The verified public key bytes (without flag byte)
    pub public_key: Vec<u8>,

    /// Signature scheme used (Ed25519, Secp256k1, Secp256r1)
    pub scheme: crate::auth::schemes::SignatureScheme,

    /// Group ID the sender is authorized for (if membership check passed)
    pub authorized_group: Option<String>,
}

/// Error types for authentication failures.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum AuthError {
    /// Signature missing from X-Signature header
    MissingSignature,

    /// Public key missing from X-Public-Key header
    MissingPublicKey,

    /// Timestamp missing from request
    MissingTimestamp,

    /// Timestamp is not a valid integer
    InvalidTimestamp(String),

    /// Request timestamp is outside the valid TTL window
    RequestExpired {
        timestamp: i64,
        server_time: i64,
        ttl_seconds: i64,
    },

    /// Signature is not valid hex encoding
    InvalidSignatureFormat(String),

    /// Public key is not valid hex encoding
    InvalidPublicKeyFormat(String),

    /// Ed25519 signature verification failed
    SignatureVerificationFailed(String),

    /// Derived address doesn't match claimed sender_address
    AddressMismatch { expected: String, got: String },

    /// Sender is not a member of the required group
    NotGroupMember { address: String, group_id: String },
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::MissingSignature => write!(f, "Missing X-Signature header"),
            AuthError::MissingPublicKey => write!(f, "Missing X-Public-Key header"),
            AuthError::MissingTimestamp => write!(f, "Missing timestamp"),
            AuthError::InvalidTimestamp(e) => write!(f, "Invalid timestamp: {}", e),
            AuthError::RequestExpired {
                timestamp,
                server_time,
                ttl_seconds,
            } => write!(
                f,
                "Request expired: timestamp {} is more than {}s from server time {}",
                timestamp, ttl_seconds, server_time
            ),
            AuthError::InvalidSignatureFormat(e) => write!(f, "Invalid signature format: {}", e),
            AuthError::InvalidPublicKeyFormat(e) => write!(f, "Invalid public key format: {}", e),
            AuthError::SignatureVerificationFailed(e) => {
                write!(f, "Signature verification failed: {}", e)
            }
            AuthError::AddressMismatch { expected, got } => {
                write!(f, "Address mismatch: expected {}, got {}", expected, got)
            }
            AuthError::NotGroupMember { address, group_id } => {
                write!(
                    f,
                    "Address {} is not a member of group {}",
                    address, group_id
                )
            }
        }
    }
}

impl std::error::Error for AuthError {}
