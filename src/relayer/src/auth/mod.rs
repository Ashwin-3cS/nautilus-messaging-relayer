//! Authentication and authorization module.
//! - Ed25519, Secp256k1, Secp256r1 signature verification
//! - Sui wallet address derivation from public keys (Blake2b-256)
//! - Permission types matching the Groups SDK smart contract
//! - Membership store trait with in-memory implementation (synced from Groups SDK events)
//! - Axum middleware for request authentication

pub mod membership;
pub mod middleware;
pub mod permissions;
pub mod schemes;
pub mod signature;
pub mod types;

#[allow(unused_imports)]
pub use membership::{
    create_membership_store, InMemoryMembershipStore, MembershipError, MembershipStore,
    MembershipStoreType,
};
pub use middleware::{auth_middleware, AuthState};
pub use permissions::MessagingPermission;
pub use types::AuthContext;
