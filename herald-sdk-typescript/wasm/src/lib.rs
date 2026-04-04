mod cipher;
mod keys;
mod veil;

use shroudb_cipher_blind::ClientKey;
use shroudb_veil_blind::{BlindKey, encode_for_wire, tokenize_and_blind};
use wasm_bindgen::prelude::*;

/// Opaque handle holding both cipher and veil keys for a session.
///
/// A session is created from a shared secret (after x25519 key exchange)
/// or restored from previously exported key bytes.
#[wasm_bindgen]
pub struct Session {
    cipher_key: ClientKey,
    veil_key: BlindKey,
}

// === Key Exchange ===

/// Generate a new x25519 keypair for E2EE key exchange.
///
/// Returns `{ publicKey: Uint8Array, secretKey: Uint8Array }`.
#[wasm_bindgen(js_name = generateKeyPair)]
pub fn generate_keypair() -> Result<JsValue, JsError> {
    let kp = keys::generate_keypair();
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(
        &obj,
        &"publicKey".into(),
        &js_sys::Uint8Array::from(&kp.public_key[..]).into(),
    )
    .map_err(|e| JsError::new(&format!("{e:?}")))?;
    js_sys::Reflect::set(
        &obj,
        &"secretKey".into(),
        &js_sys::Uint8Array::from(&kp.secret_key[..]).into(),
    )
    .map_err(|e| JsError::new(&format!("{e:?}")))?;
    Ok(obj.into())
}

/// Compute a shared secret from our secret key and their public key.
#[wasm_bindgen(js_name = deriveSharedSecret)]
pub fn derive_shared_secret(my_secret: &[u8], their_public: &[u8]) -> Result<Vec<u8>, JsError> {
    let secret: [u8; 32] = my_secret
        .try_into()
        .map_err(|_| JsError::new("secret key must be 32 bytes"))?;
    let public: [u8; 32] = their_public
        .try_into()
        .map_err(|_| JsError::new("public key must be 32 bytes"))?;
    Ok(keys::derive_shared_secret(&secret, &public).to_vec())
}

// === Session Management ===

/// Create a new E2EE session from a shared secret.
///
/// Derives both cipher and veil keys from the shared secret using
/// different HKDF domain separators.
#[wasm_bindgen(js_name = createSession)]
pub fn create_session(shared_secret: &[u8], version: u32) -> Result<Session, JsError> {
    let cipher_key = cipher::derive_key(shared_secret, version)
        .map_err(|e| JsError::new(&e.to_string()))?;
    let veil_key =
        veil::derive_key(shared_secret).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(Session {
        cipher_key,
        veil_key,
    })
}

/// Restore a session from previously exported key bytes.
#[wasm_bindgen(js_name = restoreSession)]
pub fn restore_session(
    cipher_key_bytes: &[u8],
    veil_key_bytes: &[u8],
    version: u32,
) -> Result<Session, JsError> {
    let cipher_key = cipher::key_from_bytes(cipher_key_bytes, version)
        .map_err(|e| JsError::new(&e.to_string()))?;
    let veil_key =
        veil::key_from_bytes(veil_key_bytes).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(Session {
        cipher_key,
        veil_key,
    })
}

#[wasm_bindgen]
impl Session {
    /// Export cipher key bytes for persistence.
    #[wasm_bindgen(js_name = exportCipherKey)]
    pub fn export_cipher_key(&self) -> Vec<u8> {
        self.cipher_key.as_bytes().to_vec()
    }

    /// Export veil key bytes for persistence.
    #[wasm_bindgen(js_name = exportVeilKey)]
    pub fn export_veil_key(&self) -> Vec<u8> {
        self.veil_key.as_bytes().to_vec()
    }

    /// Encrypt a plaintext string. The room ID is used as AAD (Additional
    /// Authenticated Data), binding the ciphertext to the room context.
    ///
    /// Returns a CiphertextEnvelope-formatted string.
    pub fn encrypt(&self, plaintext: &str, room_id: &str) -> Result<String, JsError> {
        self.cipher_key
            .encrypt(plaintext.as_bytes(), room_id.as_bytes())
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Decrypt a CiphertextEnvelope string. The room ID must match the
    /// value used during encryption.
    ///
    /// Returns the plaintext string.
    pub fn decrypt(&self, ciphertext: &str, room_id: &str) -> Result<String, JsError> {
        let secret = self
            .cipher_key
            .decrypt(ciphertext, room_id.as_bytes())
            .map_err(|e| JsError::new(&e.to_string()))?;
        String::from_utf8(secret.as_bytes().to_vec())
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Generate blind tokens for a plaintext string.
    ///
    /// Returns a base64-encoded wire format string suitable for
    /// storing in the message `meta.__blind` field.
    pub fn blind(&self, plaintext: &str) -> Result<String, JsError> {
        let tokens = tokenize_and_blind(&self.veil_key, plaintext);
        encode_for_wire(&tokens).map_err(|e| JsError::new(&e.to_string()))
    }
}
