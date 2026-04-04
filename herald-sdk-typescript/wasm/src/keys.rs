use x25519_dalek::{PublicKey, StaticSecret};

/// An x25519 keypair for E2EE key exchange.
pub struct KeyPair {
    pub public_key: [u8; 32],
    pub secret_key: [u8; 32],
}

/// Generate a new x25519 keypair using browser CSPRNG.
pub fn generate_keypair() -> KeyPair {
    let mut secret_bytes = [0u8; 32];
    getrandom::getrandom(&mut secret_bytes).expect("browser CSPRNG failed");
    let secret = StaticSecret::from(secret_bytes);
    let public = PublicKey::from(&secret);
    KeyPair {
        public_key: *public.as_bytes(),
        secret_key: secret.to_bytes(),
    }
}

/// Compute a shared secret from our secret key and their public key.
pub fn derive_shared_secret(my_secret: &[u8; 32], their_public: &[u8; 32]) -> [u8; 32] {
    let secret = StaticSecret::from(*my_secret);
    let public = PublicKey::from(*their_public);
    let shared = secret.diffie_hellman(&public);
    *shared.as_bytes()
}
