# frozen_string_literal: true

require "openssl"
require "json"
require "base64"

module HeraldAdmin
  # E2EE module — x25519, AES-256-GCM, HMAC-SHA256 blind tokens.
  # Uses Ruby's built-in OpenSSL bindings (Ruby >= 3.0, OpenSSL >= 1.1.1).
  module E2EE
    # CiphertextEnvelope wire format constants
    ALPHABET = "k3Xm7RqYf1LvNj9GpDw0ZhTs5CxAe4Uo8KiHb2WaSrBn6EcFdJlMgPtQyVuIzO"
    BASE = 62
    KEY_LEN = 4
    MODULUS = 14_776_336 # 62^4
    MULTIPLIER = 23_908_627
    INVERSE = 10_806_539
    ALGO_AES_256_GCM = 0
    NONCE_LEN = 12
    TAG_LEN = 16
    HKDF_CIPHER_INFO = "herald-cipher-v1"
    HKDF_VEIL_INFO = "herald-veil-v1"

    # Reverse lookup: char -> index
    ALPHA_INDEX = ALPHABET.each_char.with_index.to_h.freeze

    private_constant :ALPHABET, :BASE, :KEY_LEN, :MODULUS, :MULTIPLIER, :INVERSE,
                     :ALGO_AES_256_GCM, :NONCE_LEN, :TAG_LEN, :HKDF_CIPHER_INFO,
                     :HKDF_VEIL_INFO, :ALPHA_INDEX

    # An x25519 keypair for E2EE key exchange.
    KeyPair = Struct.new(:public_key, :secret_key, keyword_init: true)

    # Holds derived cipher and veil keys for a stream.
    class Session
      def initialize(cipher_key, veil_key, key_version)
        @cipher_key = cipher_key
        @veil_key = veil_key
        @key_version = key_version
      end

      # Return raw cipher key bytes for persistence.
      def export_cipher_key = @cipher_key

      # Return raw veil key bytes for persistence.
      def export_veil_key = @veil_key

      # Encrypt with a random nonce. stream_id is AAD.
      def encrypt(plaintext, stream_id)
        nonce = OpenSSL::Random.random_bytes(NONCE_LEN)
        seal(nonce, plaintext, stream_id)
      end

      # Encrypt with a deterministic nonce (same inputs → same output).
      def encrypt_convergent(plaintext, stream_id)
        nonce = E2EE.convergent_nonce(@cipher_key, plaintext, stream_id)
        seal(nonce, plaintext, stream_id)
      end

      # Decrypt a CiphertextEnvelope string. stream_id must match encryption.
      def decrypt(ciphertext, stream_id)
        _, payload = E2EE.decode_envelope(ciphertext)
        raise "ciphertext too short" if payload.bytesize < NONCE_LEN + TAG_LEN

        nonce = payload.byteslice(0, NONCE_LEN)
        ct_and_tag = payload.byteslice(NONCE_LEN..)
        ct = ct_and_tag.byteslice(0, ct_and_tag.bytesize - TAG_LEN)
        tag = ct_and_tag.byteslice(ct_and_tag.bytesize - TAG_LEN, TAG_LEN)

        cipher = OpenSSL::Cipher.new("aes-256-gcm")
        cipher.decrypt
        cipher.key = @cipher_key
        cipher.iv = nonce
        cipher.auth_tag = tag
        cipher.auth_data = stream_id
        (cipher.update(ct) + cipher.final).force_encoding("UTF-8")
      end

      # Generate blind search tokens. Returns base64 wire format for meta.__blind.
      def blind(plaintext)
        words, trigrams = E2EE.tokenize(plaintext)
        result = {
          words: E2EE.blind_all(@veil_key, words),
          trigrams: E2EE.blind_all(@veil_key, trigrams)
        }
        Base64.strict_encode64(JSON.generate(result))
      end

      private

      def seal(nonce, plaintext, stream_id)
        cipher = OpenSSL::Cipher.new("aes-256-gcm")
        cipher.encrypt
        cipher.key = @cipher_key
        cipher.iv = nonce
        cipher.auth_data = stream_id
        ct = cipher.update(plaintext) + cipher.final
        tag = cipher.auth_tag

        payload = nonce + ct + tag
        E2EE.encode_envelope(@key_version, ALGO_AES_256_GCM, payload)
      end
    end

    class << self
      # Generate a new x25519 keypair.
      def generate_keypair
        pkey = OpenSSL::PKey.generate_key("X25519")
        KeyPair.new(
          public_key: pkey.raw_public_key,
          secret_key: pkey.raw_private_key
        )
      end

      # Compute a shared secret from our secret key and their public key.
      def derive_shared_secret(my_secret, their_public)
        priv = OpenSSL::PKey.new_raw_private_key("X25519", my_secret)
        pub = OpenSSL::PKey.new_raw_public_key("X25519", their_public)
        priv.derive(pub)
      end

      # Derive both cipher and veil keys from a shared secret.
      def create_session(shared_secret, version)
        cipher_key = hkdf_sha256(shared_secret, HKDF_CIPHER_INFO)
        veil_key = hkdf_sha256(shared_secret, HKDF_VEIL_INFO)
        Session.new(cipher_key, veil_key, version)
      end

      # Restore a session from previously exported key bytes.
      def restore_session(cipher_key, veil_key, version)
        Session.new(cipher_key, veil_key, version)
      end

      # Decode a wire-format blind token string back to a hash.
      def decode_blind_tokens(wire)
        JSON.parse(Base64.strict_decode64(wire))
      end

      # @api private
      def hkdf_sha256(ikm, info)
        OpenSSL::KDF.hkdf(
          ikm,
          salt: "\x00" * 32,
          info: info,
          length: 32,
          hash: "SHA256"
        )
      end

      # @api private
      def convergent_nonce(key, plaintext, stream_id)
        OpenSSL::HMAC.digest("SHA256", key, plaintext + stream_id).byteslice(0, NONCE_LEN)
      end

      # @api private
      def encode_envelope(version, algo_id, payload)
        packed = (version << 4) | algo_id
        obfuscated = (packed * MULTIPLIER) % MODULUS
        prefix = encode_base62(obfuscated)
        b64 = Base64.urlsafe_encode64(payload, padding: false)
        "#{prefix}:#{b64}"
      end

      # @api private
      def decode_envelope(str)
        colon = str.index(":")
        raise "invalid ciphertext: missing ':'" unless colon
        prefix = str[0...colon]
        raise "invalid ciphertext: empty prefix" if prefix.empty?

        obfuscated = decode_base62(prefix)
        packed = (obfuscated * INVERSE) % MODULUS
        version = packed >> 4

        b64 = str[(colon + 1)..]
        payload = Base64.urlsafe_decode64(b64)
        [version, payload]
      end

      # @api private
      def encode_base62(val)
        return ALPHABET[0] * KEY_LEN if val == 0
        chars = []
        while val > 0
          chars.unshift(ALPHABET[val % BASE])
          val /= BASE
        end
        chars.unshift(ALPHABET[0]) while chars.length < KEY_LEN
        chars.join
      end

      # @api private
      def decode_base62(str)
        val = 0
        str.each_char do |c|
          idx = ALPHA_INDEX[c]
          raise "invalid character in prefix: #{c.inspect}" unless idx
          val = val * BASE + idx
        end
        val
      end

      # @api private
      def tokenize(text)
        normalized = text.downcase
        words = normalized.scan(/[a-z0-9]+/)

        word_tokens = words.map { |w| "w:#{w}" }.uniq.sort

        trigram_tokens = []
        words.each do |w|
          next if w.length < 3
          (0..w.length - 3).each { |i| trigram_tokens << "t:#{w[i, 3]}" }
        end
        trigram_tokens = trigram_tokens.uniq.sort

        [word_tokens, trigram_tokens]
      end

      # @api private
      def blind_all(key, tokens)
        tokens.map { |t| OpenSSL::HMAC.hexdigest("SHA256", key, t) }
      end
    end
  end
end
