//! Sui supports three signature schemes, each identified by a flag byte:
//! - Ed25519 (flag 0x00): 32-byte public key, 64-byte signature
//! - Secp256k1 (flag 0x01): 33-byte compressed public key, 64-byte signature
//! - Secp256r1 (flag 0x02): 33-byte compressed public key, 64-byte signature

use std::fmt;

/// Sui signature schemes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureScheme {
    Ed25519,

    Secp256k1,

    Secp256r1,
}

impl SignatureScheme {
    pub fn from_flag(flag: u8) -> Option<Self> {
        match flag {
            0x00 => Some(SignatureScheme::Ed25519),
            0x01 => Some(SignatureScheme::Secp256k1),
            0x02 => Some(SignatureScheme::Secp256r1),
            _ => None,
        }
    }

    /// Returns the flag byte for this scheme.
    pub fn flag(&self) -> u8 {
        match self {
            SignatureScheme::Ed25519 => 0x00,
            SignatureScheme::Secp256k1 => 0x01,
            SignatureScheme::Secp256r1 => 0x02,
        }
    }

    /// Returns the expected public key length in bytes.
    pub fn public_key_length(&self) -> usize {
        match self {
            SignatureScheme::Ed25519 => 32,
            SignatureScheme::Secp256k1 => 33,
            SignatureScheme::Secp256r1 => 33,
        }
    }

    /// Returns the expected signature length in bytes.
    /// All schemes use 64-byte signatures (r: 32 bytes, s: 32 bytes).
    #[allow(dead_code)]
    pub fn signature_length(&self) -> usize {
        64
    }
}

impl fmt::Display for SignatureScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignatureScheme::Ed25519 => write!(f, "Ed25519"),
            SignatureScheme::Secp256k1 => write!(f, "Secp256k1"),
            SignatureScheme::Secp256r1 => write!(f, "Secp256r1"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_flag() {
        assert_eq!(
            SignatureScheme::from_flag(0x00),
            Some(SignatureScheme::Ed25519)
        );
        assert_eq!(
            SignatureScheme::from_flag(0x01),
            Some(SignatureScheme::Secp256k1)
        );
        assert_eq!(
            SignatureScheme::from_flag(0x02),
            Some(SignatureScheme::Secp256r1)
        );
        assert_eq!(SignatureScheme::from_flag(0x03), None);
        assert_eq!(SignatureScheme::from_flag(0xFF), None);
    }

    #[test]
    fn test_flag() {
        assert_eq!(SignatureScheme::Ed25519.flag(), 0x00);
        assert_eq!(SignatureScheme::Secp256k1.flag(), 0x01);
        assert_eq!(SignatureScheme::Secp256r1.flag(), 0x02);
    }

    #[test]
    fn test_public_key_length() {
        assert_eq!(SignatureScheme::Ed25519.public_key_length(), 32);
        assert_eq!(SignatureScheme::Secp256k1.public_key_length(), 33);
        assert_eq!(SignatureScheme::Secp256r1.public_key_length(), 33);
    }
}
