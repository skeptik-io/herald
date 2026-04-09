# frozen_string_literal: true

require "minitest/autorun"
require_relative "../lib/herald_admin/e2ee"

class TestE2EE < Minitest::Test
  def test_hkdf_cipher_key
    shared = "\x42" * 32
    key = HeraldAdmin::E2EE.hkdf_sha256(shared, "herald-cipher-v1")
    assert_equal "0f1e00421174f25355b5f2608023e62ee0bd4928f89f4fb13cb22091f302e39a", key.unpack1("H*")
  end

  def test_hkdf_veil_key
    shared = "\x42" * 32
    key = HeraldAdmin::E2EE.hkdf_sha256(shared, "herald-veil-v1")
    assert_equal "63f027ee28195cf6c2e35f568a8da9baa6b05145c50a7da27bce24c80547d3cf", key.unpack1("H*")
  end

  def test_x25519_key_exchange
    alice_secret = "\x01" * 32
    bob_secret = "\x02" * 32
    alice_pub = ["a4e09292b651c278b9772c569f5fa9bb13d906b46ab68c9df9dc2b4409f8a209"].pack("H*")
    bob_pub = ["ce8d3ad1ccb633ec7b70c17814a5c76ecd029685050d344745ba05870e587d59"].pack("H*")

    alice_shared = HeraldAdmin::E2EE.derive_shared_secret(alice_secret, bob_pub)
    bob_shared = HeraldAdmin::E2EE.derive_shared_secret(bob_secret, alice_pub)

    assert_equal alice_shared, bob_shared
    assert_equal "2ed76ab549b1e73c031eb49c9448f0798aea81b698279a0c3dc3e49fbfc4b953", alice_shared.unpack1("H*")
  end

  def test_convergent_encryption
    shared = "\x42" * 32
    session = HeraldAdmin::E2EE.create_session(shared, 1)

    ct = session.encrypt_convergent("hello world", "stream-123")
    assert_equal "QRWp:mL-5BlPGudFhQ7Oiga-OEWKXny6bQ9UGlzmWeqXNSUnLfW62LbO1", ct

    pt = session.decrypt(ct, "stream-123")
    assert_equal "hello world", pt
  end

  def test_random_encrypt_decrypt
    shared = "\x42" * 32
    session = HeraldAdmin::E2EE.create_session(shared, 1)

    ct = session.encrypt("hello world", "stream-123")
    assert ct.start_with?("QRWp:")

    pt = session.decrypt(ct, "stream-123")
    assert_equal "hello world", pt
  end

  def test_decrypt_wrong_aad
    shared = "\x42" * 32
    session = HeraldAdmin::E2EE.create_session(shared, 1)

    ct = session.encrypt("hello world", "stream-123")
    assert_raises(OpenSSL::Cipher::CipherError) do
      session.decrypt(ct, "wrong-stream")
    end
  end

  def test_blind_tokens
    shared = "\x42" * 32
    session = HeraldAdmin::E2EE.create_session(shared, 1)

    wire = session.blind("Hello World")
    tokens = HeraldAdmin::E2EE.decode_blind_tokens(wire)

    want_words = [
      "64927ad8770952a8f4eb5c01e9365d9c29f1de103516007ed26a0aa38f4c0bc4",
      "33fd718299d726735b90361b89bf131b624e83d5e30165f888a50b220c5c0e53"
    ]
    want_trigrams = [
      "159950c56432b88c3d1e98fd1b41a86dd43de392327637758a3b713830b191bf",
      "1f7ddfd1f36053b7646f4bb5e0acf4b8fbd5a741f0565839a4def87d23f7d33e",
      "dd27dab20b0561256c2795345d2a78e1997dbf539f24f0879571f65fdf3058d2",
      "bcf5d64bc6c25f53fbbef004bd668f4615007c446cf23657c401e63a0c39b2ff",
      "681173c3117c9e51782ba884c5fc5d9dd8b0893ec70d5f29aeef12c88004611b",
      "5e69b89485f166cb35dcae314549dad5a7aa88b4b2aaab891fd274eb4f684967"
    ]

    assert_equal want_words, tokens["words"]
    assert_equal want_trigrams, tokens["trigrams"]
  end

  def test_envelope_prefix_roundtrip
    cases = [
      [0, 0, "kkkk"],
      [1, 0, "QRWp"],
      [2, 0, "dv98"],
      [3, 0, "rpgd"],
      [0, 1, "W0E3"],
      [1, 1, "oChD"]
    ]

    modulus = 14_776_336
    mult = 23_908_627
    inverse = 10_806_539

    cases.each do |version, algo_id, want_prefix|
      packed = (version << 4) | algo_id
      obfuscated = (packed * mult) % modulus
      prefix = HeraldAdmin::E2EE.encode_base62(obfuscated)
      assert_equal want_prefix, prefix, "v=#{version} a=#{algo_id}"

      decoded = HeraldAdmin::E2EE.decode_base62(prefix)
      decoded_packed = (decoded * inverse) % modulus
      assert_equal version, decoded_packed >> 4
      assert_equal algo_id, decoded_packed & 0xF
    end
  end

  def test_session_export_restore
    shared = "\x42" * 32
    session = HeraldAdmin::E2EE.create_session(shared, 1)

    ct = session.encrypt_convergent("test message", "room-1")

    restored = HeraldAdmin::E2EE.restore_session(
      session.export_cipher_key, session.export_veil_key, 1
    )

    pt = restored.decrypt(ct, "room-1")
    assert_equal "test message", pt

    assert_equal session.blind("test"), restored.blind("test")
  end

  def test_tokenize
    words, trigrams = HeraldAdmin::E2EE.tokenize("Hello World")
    assert_equal ["w:hello", "w:world"], words
    assert_equal ["t:ell", "t:hel", "t:llo", "t:orl", "t:rld", "t:wor"], trigrams
  end

  def test_generate_keypair
    kp = HeraldAdmin::E2EE.generate_keypair
    assert_equal 32, kp.public_key.bytesize
    assert_equal 32, kp.secret_key.bytesize
    refute_equal kp.public_key, kp.secret_key
  end
end
