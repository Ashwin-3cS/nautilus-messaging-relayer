//! Signature verification and Sui address derivation
//! Supports all three Sui signature schemes:
//! - Ed25519 (flag 0x00): 32-byte public key
//! - Secp256k1 (flag 0x01): 33-byte compressed public key
//! - Secp256r1 (flag 0x02): 33-byte compressed public key

use blake2::{digest::consts::U32, Blake2b, Digest};
use std::borrow::Cow;
use sui_crypto::{SuiVerifier, UserSignatureVerifier};
use sui_sdk_types::{PersonalMessage, UserSignature};

use super::schemes::SignatureScheme;
use super::types::AuthError;

/// Default TTL for request timestamps (5 minutes).
#[allow(dead_code)]
pub const DEFAULT_REQUEST_TTL_SECONDS: i64 = 300;

/// Validates that a timestamp is within the acceptable TTL window.
pub fn validate_timestamp(timestamp: i64, ttl_seconds: i64) -> Result<(), AuthError> {
    let now = chrono::Utc::now().timestamp();

    let diff = (now - timestamp).abs();

    if diff > ttl_seconds {
        return Err(AuthError::RequestExpired {
            timestamp,
            server_time: now,
            ttl_seconds,
        });
    }

    Ok(())
}

/// Verifies a signature against a message
pub fn verify_signature(
    message: &[u8],
    signature_bytes: &[u8],
    public_key_bytes: &[u8],
    scheme: SignatureScheme,
) -> Result<(), AuthError> {
    if signature_bytes.len() != 64 {
        return Err(AuthError::InvalidSignatureFormat(format!(
            "Expected 64 bytes, got {}",
            signature_bytes.len()
        )));
    }

    let expected_len = scheme.public_key_length();
    if public_key_bytes.len() != expected_len {
        return Err(AuthError::InvalidPublicKeyFormat(format!(
            "Expected {} bytes for {}, got {}",
            expected_len,
            scheme,
            public_key_bytes.len()
        )));
    }

    let mut serialized_sig = Vec::with_capacity(1 + 64 + expected_len);
    serialized_sig.push(scheme.flag());
    serialized_sig.extend_from_slice(signature_bytes);
    serialized_sig.extend_from_slice(public_key_bytes);

    let user_signature = UserSignature::from_bytes(&serialized_sig).map_err(|e| {
        AuthError::InvalidSignatureFormat(format!("Failed to parse signature: {}", e))
    })?;

    let personal_message = PersonalMessage(Cow::Borrowed(message));

    let verifier = UserSignatureVerifier::default();

    verifier
        .verify_personal_message(&personal_message, &user_signature)
        .map_err(|e| AuthError::SignatureVerificationFailed(e.to_string()))?;

    Ok(())
}

/// Derives a Sui address from a public key and scheme.
/// Uses Blake2b-256 hash of (flag || public_key).
pub fn derive_sui_address(
    public_key_bytes: &[u8],
    scheme: SignatureScheme,
) -> Result<String, AuthError> {
    // Validate public key length
    let expected_len = scheme.public_key_length();
    if public_key_bytes.len() != expected_len {
        return Err(AuthError::InvalidPublicKeyFormat(format!(
            "Expected {} bytes for {}, got {}",
            expected_len,
            scheme,
            public_key_bytes.len()
        )));
    }

    // Build the hash input: flag || public_key
    let mut hash_input = vec![scheme.flag()];
    hash_input.extend_from_slice(public_key_bytes);

    // Hash with Blake2b-256 to derive the address
    type Blake2b256 = Blake2b<U32>;
    let hash = Blake2b256::digest(&hash_input);

    // Return as hex string with 0x prefix
    Ok(format!("0x{}", hex::encode(hash)))
}

/// Verifies that the claimed address matches the public key.
pub fn verify_address_matches_pubkey(
    claimed_address: &str,
    public_key_bytes: &[u8],
    scheme: SignatureScheme,
) -> Result<String, AuthError> {
    let derived_address = derive_sui_address(public_key_bytes, scheme)?;

    if claimed_address != derived_address {
        return Err(AuthError::AddressMismatch {
            expected: derived_address.clone(),
            got: claimed_address.to_string(),
        });
    }

    Ok(derived_address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sui_crypto::{
        ed25519::Ed25519PrivateKey, secp256k1::Secp256k1PrivateKey, secp256r1::Secp256r1PrivateKey,
        SuiSigner,
    };

    /// Extract raw 64-byte signature from UserSignature bytes.
    /// Format: flag (1 byte) || signature (64 bytes) || public_key
    fn extract_signature_bytes(user_sig_bytes: &[u8]) -> Vec<u8> {
        user_sig_bytes[1..65].to_vec()
    }

    /// Test Ed25519 signature verification with a real signature
    #[test]
    fn test_verify_signature_ed25519() {
        let private_key_hex = "4ac9bd5399f7b41da4f00ec612c4e6521a1c756c41578ed5c15133f96ab9ea78";
        let public_key_hex = "dec9c24a98da1187e30a5824ca2ee1e91e956b7dd6970590651d7d46c5e2ed41";

        let private_key_bytes: [u8; 32] = hex::decode(private_key_hex).unwrap().try_into().unwrap();
        let public_key_bytes = hex::decode(public_key_hex).unwrap();

        // Create signing key using sui-crypto's Ed25519PrivateKey
        let signing_key = Ed25519PrivateKey::new(private_key_bytes);
        let message = b"test message";

        // Sign using SuiSigner trait (handles personal message format internally)
        let personal_message = PersonalMessage(Cow::Borrowed(message.as_slice()));
        let user_signature = signing_key
            .sign_personal_message(&personal_message)
            .unwrap();
        let signature_bytes = extract_signature_bytes(&user_signature.to_bytes());

        // Verify using our function
        let result = verify_signature(
            message,
            &signature_bytes,
            &public_key_bytes,
            SignatureScheme::Ed25519,
        );
        assert!(result.is_ok(), "Ed25519 verification failed: {:?}", result);
    }

    /// Test Secp256k1 signature verification with a real signature
    #[test]
    fn test_verify_signature_secp256k1() {
        let private_key_hex = "6ae98ba75c281c5ea3fb80f06f5f1afd8a6b69ec2a02186c73c928d67c96cd4b";

        let private_key_bytes: [u8; 32] = hex::decode(private_key_hex).unwrap().try_into().unwrap();

        // Create signing key using sui-crypto's Secp256k1PrivateKey
        let signing_key = Secp256k1PrivateKey::new(private_key_bytes).unwrap();
        let public_key_bytes = signing_key.public_key().as_bytes().to_vec();

        let message = b"test message";

        // Sign using SuiSigner trait
        let personal_message = PersonalMessage(Cow::Borrowed(message.as_slice()));
        let user_signature = signing_key
            .sign_personal_message(&personal_message)
            .unwrap();
        let signature_bytes = extract_signature_bytes(&user_signature.to_bytes());

        let result = verify_signature(
            message,
            &signature_bytes,
            &public_key_bytes,
            SignatureScheme::Secp256k1,
        );
        assert!(
            result.is_ok(),
            "Secp256k1 verification failed: {:?}",
            result
        );
    }

    /// Test Secp256r1 signature verification
    #[test]
    fn test_verify_signature_secp256r1() {
        let private_key_hex = "7e944e7562603f3a6a0d799ca760d9e113de997da5b6915f70716fb371efae90";

        let private_key_bytes: [u8; 32] = hex::decode(private_key_hex).unwrap().try_into().unwrap();

        let signing_key = Secp256r1PrivateKey::new(private_key_bytes);
        let public_key_bytes = signing_key.public_key().as_bytes().to_vec();

        let message = b"test message";

        let personal_message = PersonalMessage(Cow::Borrowed(message.as_slice()));
        let user_signature = signing_key
            .sign_personal_message(&personal_message)
            .unwrap();
        let signature_bytes = extract_signature_bytes(&user_signature.to_bytes());

        let result = verify_signature(
            message,
            &signature_bytes,
            &public_key_bytes,
            SignatureScheme::Secp256r1,
        );
        assert!(
            result.is_ok(),
            "Secp256r1 verification failed: {:?}",
            result
        );
    }

    // ==================== Ed25519 Tests ====================

    /// Test Ed25519 address derivation
    #[test]
    fn test_derive_sui_address_ed25519() {
        let public_key_hex = "dec9c24a98da1187e30a5824ca2ee1e91e956b7dd6970590651d7d46c5e2ed41";
        let public_key_bytes = hex::decode(public_key_hex).unwrap();
        let expected_address = "0xc45d73cf687682db23be0ebdef5bc203585315b2d6a5a6a613b941e4d4a6a0e7";

        let derived = derive_sui_address(&public_key_bytes, SignatureScheme::Ed25519).unwrap();
        assert_eq!(derived, expected_address);
    }

    #[test]
    fn test_verify_address_matches_ed25519() {
        let public_key_hex = "dec9c24a98da1187e30a5824ca2ee1e91e956b7dd6970590651d7d46c5e2ed41";
        let public_key_bytes = hex::decode(public_key_hex).unwrap();
        let expected_address = "0xc45d73cf687682db23be0ebdef5bc203585315b2d6a5a6a613b941e4d4a6a0e7";

        let result = verify_address_matches_pubkey(
            expected_address,
            &public_key_bytes,
            SignatureScheme::Ed25519,
        );
        assert!(result.is_ok());
    }

    // ==================== Secp256k1 Tests ====================

    /// Test Secp256k1 address derivation
    #[test]
    fn test_derive_sui_address_secp256k1() {
        let public_key_hex = "024324a9c68113352194ff0b8bca673e6d01f67e97f80a827ee9ce898119da9f86";
        let public_key_bytes = hex::decode(public_key_hex).unwrap();
        let expected_address = "0x87ee5d74c3e7ae5145072943685451dfd71a8e911c04f0d90e636ec7d6483543";

        let derived = derive_sui_address(&public_key_bytes, SignatureScheme::Secp256k1).unwrap();
        assert_eq!(derived, expected_address);
    }

    #[test]
    fn test_verify_address_matches_secp256k1() {
        let public_key_hex = "024324a9c68113352194ff0b8bca673e6d01f67e97f80a827ee9ce898119da9f86";
        let public_key_bytes = hex::decode(public_key_hex).unwrap();
        let expected_address = "0x87ee5d74c3e7ae5145072943685451dfd71a8e911c04f0d90e636ec7d6483543";

        let result = verify_address_matches_pubkey(
            expected_address,
            &public_key_bytes,
            SignatureScheme::Secp256k1,
        );
        assert!(result.is_ok());
    }

    // ==================== Secp256r1 Tests ====================

    /// Test Secp256r1 address derivation
    #[test]
    fn test_derive_sui_address_secp256r1() {
        let public_key_hex = "027951b52f60955a34eaac3bb75d086d1c431e45a9b44d0730d29db84ec148511e";
        let public_key_bytes = hex::decode(public_key_hex).unwrap();
        let expected_address = "0x1f4283b353e5d5086bff6b7b68c4149a8c284fa53b0ca34a48cdfb407c6c2c09";

        let derived = derive_sui_address(&public_key_bytes, SignatureScheme::Secp256r1).unwrap();
        assert_eq!(derived, expected_address);
    }

    #[test]
    fn test_verify_address_matches_secp256r1() {
        let public_key_hex = "027951b52f60955a34eaac3bb75d086d1c431e45a9b44d0730d29db84ec148511e";
        let public_key_bytes = hex::decode(public_key_hex).unwrap();
        let expected_address = "0x1f4283b353e5d5086bff6b7b68c4149a8c284fa53b0ca34a48cdfb407c6c2c09";

        let result = verify_address_matches_pubkey(
            expected_address,
            &public_key_bytes,
            SignatureScheme::Secp256r1,
        );
        assert!(result.is_ok());
    }

    // ==================== Address Mismatch Test ====================

    #[test]
    fn test_verify_address_mismatch() {
        let public_key_hex = "dec9c24a98da1187e30a5824ca2ee1e91e956b7dd6970590651d7d46c5e2ed41";
        let public_key_bytes = hex::decode(public_key_hex).unwrap();
        let wrong_address = "0x1f4283b353e5d5086bff6b7b68c4149a8c284fa53b0ca34a48cdfb407c6c2c10";

        let result = verify_address_matches_pubkey(
            wrong_address,
            &public_key_bytes,
            SignatureScheme::Ed25519,
        );
        assert!(result.is_err());
    }

    // ==================== Timestamp Tests ====================

    #[test]
    fn test_validate_timestamp_valid() {
        let now = chrono::Utc::now().timestamp();
        let ttl = 300;

        assert!(validate_timestamp(now, ttl).is_ok());
        assert!(validate_timestamp(now - 60, ttl).is_ok());
        assert!(validate_timestamp(now + 60, ttl).is_ok());
    }

    #[test]
    fn test_validate_timestamp_expired() {
        let now = chrono::Utc::now().timestamp();
        let ttl = 300;

        assert!(validate_timestamp(now - 600, ttl).is_err());
        assert!(validate_timestamp(now + 600, ttl).is_err());
    }
}
