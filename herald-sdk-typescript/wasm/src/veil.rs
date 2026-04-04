use shroudb_veil_blind::{BlindError, BlindKey};

/// Derive a veil key from a shared secret.
///
/// Uses HKDF-SHA256 with `b"herald-veil-v1"` as the info parameter,
/// producing a key deterministically bound to this domain.
pub fn derive_key(shared_secret: &[u8]) -> Result<BlindKey, BlindError> {
    BlindKey::derive(shared_secret, b"herald-veil-v1")
}

/// Restore a veil key from raw bytes.
pub fn key_from_bytes(key_bytes: &[u8]) -> Result<BlindKey, BlindError> {
    BlindKey::from_bytes(key_bytes.to_vec())
}
