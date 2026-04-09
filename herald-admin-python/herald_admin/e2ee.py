"""Herald E2EE module — x25519, AES-256-GCM, HMAC-SHA256 blind tokens.

Requires the ``cryptography`` package::

    pip install cryptography

Or install with the e2ee extra::

    pip install herald-admin[e2ee]
"""
from __future__ import annotations

import base64
import hashlib
import hmac
import json
import os
import re
from dataclasses import dataclass

try:
    from cryptography.hazmat.primitives.asymmetric.x25519 import (
        X25519PrivateKey,
        X25519PublicKey,
    )
    from cryptography.hazmat.primitives.ciphers.aead import AESGCM
except ImportError as exc:
    raise ImportError(
        "herald E2EE requires the 'cryptography' package: pip install cryptography"
    ) from exc

# CiphertextEnvelope wire format constants
_ALPHABET = "k3Xm7RqYf1LvNj9GpDw0ZhTs5CxAe4Uo8KiHb2WaSrBn6EcFdJlMgPtQyVuIzO"
_BASE = 62
_KEY_LEN = 4
_MODULUS = 14_776_336  # 62^4
_MULTIPLIER = 23_908_627
_INVERSE = 10_806_539
_ALGO_AES_256_GCM = 0
_NONCE_LEN = 12
_TAG_LEN = 16
_HKDF_CIPHER_INFO = b"herald-cipher-v1"
_HKDF_VEIL_INFO = b"herald-veil-v1"

# Reverse lookup: char -> index
_ALPHA_INDEX = {c: i for i, c in enumerate(_ALPHABET)}


@dataclass
class E2EEKeyPair:
    """An x25519 keypair for E2EE key exchange."""
    public_key: bytes  # 32 bytes
    secret_key: bytes  # 32 bytes


class E2EESession:
    """Holds derived cipher and veil keys for a stream."""

    __slots__ = ("_cipher_key", "_veil_key", "_key_version")

    def __init__(self, cipher_key: bytes, veil_key: bytes, key_version: int) -> None:
        self._cipher_key = cipher_key
        self._veil_key = veil_key
        self._key_version = key_version

    def export_cipher_key(self) -> bytes:
        """Return raw cipher key bytes for persistence."""
        return self._cipher_key

    def export_veil_key(self) -> bytes:
        """Return raw veil key bytes for persistence."""
        return self._veil_key

    def encrypt(self, plaintext: str, stream_id: str) -> str:
        """Encrypt with a random nonce. stream_id is AAD."""
        nonce = os.urandom(_NONCE_LEN)
        return self._seal(nonce, plaintext.encode(), stream_id.encode())

    def encrypt_convergent(self, plaintext: str, stream_id: str) -> str:
        """Encrypt with a deterministic nonce (same inputs → same output)."""
        pt = plaintext.encode()
        aad = stream_id.encode()
        nonce = _convergent_nonce(self._cipher_key, pt, aad)
        return self._seal(nonce, pt, aad)

    def decrypt(self, ciphertext: str, stream_id: str) -> str:
        """Decrypt a CiphertextEnvelope string. stream_id must match encryption."""
        _, payload = _decode_envelope(ciphertext)
        if len(payload) < _NONCE_LEN + _TAG_LEN:
            raise ValueError("ciphertext too short")
        nonce = payload[:_NONCE_LEN]
        ct_and_tag = payload[_NONCE_LEN:]
        aesgcm = AESGCM(self._cipher_key)
        pt = aesgcm.decrypt(nonce, ct_and_tag, stream_id.encode())
        return pt.decode()

    def blind(self, plaintext: str) -> str:
        """Generate blind search tokens. Returns base64 wire format for meta.__blind."""
        tokens = _tokenize(plaintext)
        result = {
            "words": _blind_all(self._veil_key, tokens[0]),
            "trigrams": _blind_all(self._veil_key, tokens[1]),
        }
        return base64.b64encode(json.dumps(result, separators=(",", ":")).encode()).decode()

    def _seal(self, nonce: bytes, plaintext: bytes, aad: bytes) -> str:
        aesgcm = AESGCM(self._cipher_key)
        ct_and_tag = aesgcm.encrypt(nonce, plaintext, aad)
        payload = nonce + ct_and_tag
        return _encode_envelope(self._key_version, _ALGO_AES_256_GCM, payload)


def generate_keypair() -> E2EEKeyPair:
    """Generate a new x25519 keypair."""
    private = X25519PrivateKey.generate()
    secret_bytes = private.private_bytes_raw()
    public_bytes = private.public_key().public_bytes_raw()
    return E2EEKeyPair(public_key=public_bytes, secret_key=secret_bytes)


def derive_shared_secret(my_secret: bytes, their_public: bytes) -> bytes:
    """Compute a shared secret from our secret key and their public key."""
    private = X25519PrivateKey.from_private_bytes(my_secret)
    public = X25519PublicKey.from_public_bytes(their_public)
    return private.exchange(public)


def create_session(shared_secret: bytes, version: int) -> E2EESession:
    """Derive both cipher and veil keys from a shared secret."""
    cipher_key = _hkdf_sha256(shared_secret, _HKDF_CIPHER_INFO)
    veil_key = _hkdf_sha256(shared_secret, _HKDF_VEIL_INFO)
    return E2EESession(cipher_key, veil_key, version)


def restore_session(cipher_key: bytes, veil_key: bytes, version: int) -> E2EESession:
    """Restore a session from previously exported key bytes."""
    return E2EESession(cipher_key, veil_key, version)


def decode_blind_tokens(wire: str) -> dict:
    """Decode a wire-format blind token string back to a dict with words/trigrams."""
    return json.loads(base64.b64decode(wire))


# --- internal helpers ---


def _hkdf_sha256(ikm: bytes, info: bytes) -> bytes:
    """HKDF-SHA256 with empty (all-zeros) salt, 32-byte output."""
    # Extract: PRK = HMAC-SHA256(salt=zeros(32), IKM)
    prk = hmac.new(bytes(32), ikm, hashlib.sha256).digest()
    # Expand: OKM = HMAC-SHA256(PRK, info || 0x01)
    okm = hmac.new(prk, info + b"\x01", hashlib.sha256).digest()
    return okm


def _convergent_nonce(key: bytes, plaintext: bytes, aad: bytes) -> bytes:
    """Deterministic 12-byte nonce from HMAC-SHA256(key, plaintext||aad)."""
    return hmac.new(key, plaintext + aad, hashlib.sha256).digest()[:_NONCE_LEN]


def _encode_envelope(version: int, algo_id: int, payload: bytes) -> str:
    packed = (version << 4) | algo_id
    obfuscated = (packed * _MULTIPLIER) % _MODULUS
    prefix = _encode_base62(obfuscated)
    b64 = base64.urlsafe_b64encode(payload).rstrip(b"=").decode()
    return f"{prefix}:{b64}"


def _decode_envelope(s: str) -> tuple[int, bytes]:
    colon = s.find(":")
    if colon < 0:
        raise ValueError("invalid ciphertext: missing ':'")
    prefix = s[:colon]
    if not prefix:
        raise ValueError("invalid ciphertext: empty prefix")
    obfuscated = _decode_base62(prefix)
    packed = (obfuscated * _INVERSE) % _MODULUS
    version = packed >> 4
    # Restore base64url padding
    b64 = s[colon + 1:]
    padding = (4 - len(b64) % 4) % 4
    b64 += "=" * padding
    payload = base64.urlsafe_b64decode(b64)
    return version, payload


def _encode_base62(val: int) -> str:
    if val == 0:
        return _ALPHABET[0] * _KEY_LEN
    chars: list[str] = []
    while val > 0:
        chars.append(_ALPHABET[val % _BASE])
        val //= _BASE
    chars.reverse()
    while len(chars) < _KEY_LEN:
        chars.insert(0, _ALPHABET[0])
    return "".join(chars)


def _decode_base62(s: str) -> int:
    val = 0
    for c in s:
        idx = _ALPHA_INDEX.get(c)
        if idx is None:
            raise ValueError(f"invalid character in prefix: {c!r}")
        val = val * _BASE + idx
    return val


def _tokenize(text: str) -> tuple[list[str], list[str]]:
    """Tokenize text into word and trigram tokens."""
    normalized = text.lower()
    words = [w for w in re.split(r"[^a-z0-9]+", normalized) if w]

    word_tokens = sorted(set(f"w:{w}" for w in words))

    trigram_set: set[str] = set()
    for w in words:
        if len(w) >= 3:
            for i in range(len(w) - 2):
                trigram_set.add(f"t:{w[i:i+3]}")
    trigram_tokens = sorted(trigram_set)

    return word_tokens, trigram_tokens


def _blind_all(key: bytes, tokens: list[str]) -> list[str]:
    return [hmac.new(key, t.encode(), hashlib.sha256).hexdigest() for t in tokens]
