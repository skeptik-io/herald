"""E2EE test suite — test vectors from the Rust shroudb-cipher-blind / shroudb-veil-blind crates."""
from herald_admin.e2ee import (
    E2EESession,
    create_session,
    decode_blind_tokens,
    derive_shared_secret,
    generate_keypair,
    restore_session,
    _decode_base62,
    _encode_base62,
    _hkdf_sha256,
    _tokenize,
)


def test_hkdf_cipher_key():
    shared = bytes([0x42] * 32)
    key = _hkdf_sha256(shared, b"herald-cipher-v1")
    assert key.hex() == "0f1e00421174f25355b5f2608023e62ee0bd4928f89f4fb13cb22091f302e39a"


def test_hkdf_veil_key():
    shared = bytes([0x42] * 32)
    key = _hkdf_sha256(shared, b"herald-veil-v1")
    assert key.hex() == "63f027ee28195cf6c2e35f568a8da9baa6b05145c50a7da27bce24c80547d3cf"


def test_x25519_key_exchange():
    alice_secret = bytes([1] * 32)
    bob_secret = bytes([2] * 32)

    alice_pub = bytes.fromhex("a4e09292b651c278b9772c569f5fa9bb13d906b46ab68c9df9dc2b4409f8a209")
    bob_pub = bytes.fromhex("ce8d3ad1ccb633ec7b70c17814a5c76ecd029685050d344745ba05870e587d59")

    alice_shared = derive_shared_secret(alice_secret, bob_pub)
    bob_shared = derive_shared_secret(bob_secret, alice_pub)

    assert alice_shared == bob_shared
    assert alice_shared.hex() == "2ed76ab549b1e73c031eb49c9448f0798aea81b698279a0c3dc3e49fbfc4b953"


def test_convergent_encryption():
    shared = bytes([0x42] * 32)
    session = create_session(shared, 1)

    ct = session.encrypt_convergent("hello world", "stream-123")
    assert ct == "QRWp:mL-5BlPGudFhQ7Oiga-OEWKXny6bQ9UGlzmWeqXNSUnLfW62LbO1"

    pt = session.decrypt(ct, "stream-123")
    assert pt == "hello world"


def test_random_encrypt_decrypt():
    shared = bytes([0x42] * 32)
    session = create_session(shared, 1)

    ct = session.encrypt("hello world", "stream-123")
    assert ct.startswith("QRWp:")

    pt = session.decrypt(ct, "stream-123")
    assert pt == "hello world"


def test_decrypt_wrong_aad():
    shared = bytes([0x42] * 32)
    session = create_session(shared, 1)

    ct = session.encrypt("hello world", "stream-123")
    try:
        session.decrypt(ct, "wrong-stream")
        assert False, "expected decryption to fail"
    except Exception:
        pass


def test_blind_tokens():
    shared = bytes([0x42] * 32)
    session = create_session(shared, 1)

    wire = session.blind("Hello World")
    tokens = decode_blind_tokens(wire)

    want_words = [
        "64927ad8770952a8f4eb5c01e9365d9c29f1de103516007ed26a0aa38f4c0bc4",
        "33fd718299d726735b90361b89bf131b624e83d5e30165f888a50b220c5c0e53",
    ]
    want_trigrams = [
        "159950c56432b88c3d1e98fd1b41a86dd43de392327637758a3b713830b191bf",
        "1f7ddfd1f36053b7646f4bb5e0acf4b8fbd5a741f0565839a4def87d23f7d33e",
        "dd27dab20b0561256c2795345d2a78e1997dbf539f24f0879571f65fdf3058d2",
        "bcf5d64bc6c25f53fbbef004bd668f4615007c446cf23657c401e63a0c39b2ff",
        "681173c3117c9e51782ba884c5fc5d9dd8b0893ec70d5f29aeef12c88004611b",
        "5e69b89485f166cb35dcae314549dad5a7aa88b4b2aaab891fd274eb4f684967",
    ]

    assert tokens["words"] == want_words
    assert tokens["trigrams"] == want_trigrams


def test_envelope_prefix_roundtrip():
    cases = [
        (0, 0, "kkkk"),
        (1, 0, "QRWp"),
        (2, 0, "dv98"),
        (3, 0, "rpgd"),
        (0, 1, "W0E3"),
        (1, 1, "oChD"),
    ]
    modulus = 14_776_336
    mult = 23_908_627
    inverse = 10_806_539

    for version, algo_id, want_prefix in cases:
        packed = (version << 4) | algo_id
        obfuscated = (packed * mult) % modulus
        prefix = _encode_base62(obfuscated)
        assert prefix == want_prefix, f"v={version} a={algo_id}: {prefix!r} != {want_prefix!r}"

        decoded = _decode_base62(prefix)
        decoded_packed = (decoded * inverse) % modulus
        assert decoded_packed >> 4 == version
        assert decoded_packed & 0xF == algo_id


def test_session_export_restore():
    shared = bytes([0x42] * 32)
    session = create_session(shared, 1)

    ct = session.encrypt_convergent("test message", "room-1")

    restored = restore_session(session.export_cipher_key(), session.export_veil_key(), 1)
    pt = restored.decrypt(ct, "room-1")
    assert pt == "test message"

    assert session.blind("test") == restored.blind("test")


def test_tokenize():
    words, trigrams = _tokenize("Hello World")
    assert words == ["w:hello", "w:world"]
    assert trigrams == ["t:ell", "t:hel", "t:llo", "t:orl", "t:rld", "t:wor"]


def test_generate_keypair():
    kp = generate_keypair()
    assert len(kp.public_key) == 32
    assert len(kp.secret_key) == 32
    assert kp.public_key != kp.secret_key
