use shroudb_cipher_blind::{Algorithm, CipherBlindError, ClientKey};

/// Derive a cipher key from a shared secret.
///
/// Uses HKDF-SHA256 with `b"herald-cipher-v1"` as the info parameter,
/// producing a key deterministically bound to this domain.
pub fn derive_key(shared_secret: &[u8], version: u32) -> Result<ClientKey, CipherBlindError> {
    ClientKey::derive(Algorithm::Aes256Gcm, shared_secret, b"herald-cipher-v1", version)
}

/// Restore a cipher key from raw bytes.
pub fn key_from_bytes(key_bytes: &[u8], version: u32) -> Result<ClientKey, CipherBlindError> {
    ClientKey::from_bytes(Algorithm::Aes256Gcm, key_bytes.to_vec(), version)
}
