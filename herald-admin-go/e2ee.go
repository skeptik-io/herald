package herald

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"sort"
	"strings"
	"unicode"

	"golang.org/x/crypto/curve25519"
)

// E2EE errors.
var (
	ErrInvalidKeyLength   = errors.New("herald: key must be 32 bytes")
	ErrInvalidCiphertext  = errors.New("herald: invalid ciphertext format")
	ErrDecryptionFailed   = errors.New("herald: decryption failed")
	ErrCiphertextTooShort = errors.New("herald: ciphertext too short")
)

// CiphertextEnvelope wire format constants.
const (
	envelopeAlphabet = "k3Xm7RqYf1LvNj9GpDw0ZhTs5CxAe4Uo8KiHb2WaSrBn6EcFdJlMgPtQyVuIzO"
	envelopeBase     = 62
	envelopeKeyLen   = 4
	envelopeModulus  = 14776336 // 62^4
	obfuskeyMult     = 23908627
	obfuskeyInverse  = 10806539

	algoAES256GCM = 0

	nonceLen = 12
	tagLen   = 16

	hkdfCipherInfo = "herald-cipher-v1"
	hkdfVeilInfo   = "herald-veil-v1"
)

// lookup table: char -> index in envelope alphabet
var alphabetIndex [256]int

func init() {
	for i := range alphabetIndex {
		alphabetIndex[i] = -1
	}
	for i, c := range envelopeAlphabet {
		alphabetIndex[c] = i
	}
}

// E2EEKeyPair holds an x25519 keypair for E2EE key exchange.
type E2EEKeyPair struct {
	PublicKey [32]byte
	SecretKey [32]byte
}

// E2EESession holds derived cipher and veil keys for a stream.
type E2EESession struct {
	cipherKey  [32]byte
	veilKey    [32]byte
	keyVersion uint32
}

// GenerateKeyPair creates a new x25519 keypair using a CSPRNG.
func GenerateKeyPair() (*E2EEKeyPair, error) {
	var secret [32]byte
	if _, err := rand.Read(secret[:]); err != nil {
		return nil, fmt.Errorf("herald: generate keypair: %w", err)
	}
	pub, err := curve25519.X25519(secret[:], curve25519.Basepoint)
	if err != nil {
		return nil, fmt.Errorf("herald: derive public key: %w", err)
	}
	kp := &E2EEKeyPair{SecretKey: secret}
	copy(kp.PublicKey[:], pub)
	return kp, nil
}

// DeriveSharedSecret computes a shared secret from our secret key and their public key.
func DeriveSharedSecret(mySecret, theirPublic [32]byte) ([32]byte, error) {
	shared, err := curve25519.X25519(mySecret[:], theirPublic[:])
	if err != nil {
		return [32]byte{}, fmt.Errorf("herald: derive shared secret: %w", err)
	}
	var out [32]byte
	copy(out[:], shared)
	return out, nil
}

// CreateSession derives both cipher and veil keys from a shared secret.
func CreateSession(sharedSecret [32]byte, version uint32) *E2EESession {
	s := &E2EESession{keyVersion: version}
	s.cipherKey = hkdfSHA256(sharedSecret[:], []byte(hkdfCipherInfo))
	s.veilKey = hkdfSHA256(sharedSecret[:], []byte(hkdfVeilInfo))
	return s
}

// RestoreSession restores a session from previously exported key bytes.
func RestoreSession(cipherKey, veilKey [32]byte, version uint32) *E2EESession {
	return &E2EESession{
		cipherKey:  cipherKey,
		veilKey:    veilKey,
		keyVersion: version,
	}
}

// ExportCipherKey returns the raw cipher key bytes for persistence.
func (s *E2EESession) ExportCipherKey() [32]byte { return s.cipherKey }

// ExportVeilKey returns the raw veil key bytes for persistence.
func (s *E2EESession) ExportVeilKey() [32]byte { return s.veilKey }

// Encrypt encrypts plaintext with a random nonce. The streamID is used as AAD.
func (s *E2EESession) Encrypt(plaintext, streamID string) (string, error) {
	var nonce [nonceLen]byte
	if _, err := rand.Read(nonce[:]); err != nil {
		return "", fmt.Errorf("herald: nonce generation: %w", err)
	}
	return s.sealWithNonce(nonce, []byte(plaintext), []byte(streamID))
}

// EncryptConvergent encrypts plaintext with a deterministic nonce derived from
// HMAC-SHA256(key, plaintext||aad). Same inputs always produce the same ciphertext.
func (s *E2EESession) EncryptConvergent(plaintext, streamID string) (string, error) {
	pt := []byte(plaintext)
	aad := []byte(streamID)
	nonce := convergentNonce(s.cipherKey[:], pt, aad)
	return s.sealWithNonce(nonce, pt, aad)
}

// Decrypt decrypts a CiphertextEnvelope string. The streamID must match encryption.
func (s *E2EESession) Decrypt(ciphertext, streamID string) (string, error) {
	_, payload, err := decodeEnvelope(ciphertext)
	if err != nil {
		return "", err
	}
	if len(payload) < nonceLen+tagLen {
		return "", ErrCiphertextTooShort
	}

	nonce := payload[:nonceLen]
	encrypted := payload[nonceLen:]

	block, err := aes.NewCipher(s.cipherKey[:])
	if err != nil {
		return "", fmt.Errorf("herald: aes init: %w", err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return "", fmt.Errorf("herald: gcm init: %w", err)
	}
	pt, err := gcm.Open(nil, nonce, encrypted, []byte(streamID))
	if err != nil {
		return "", ErrDecryptionFailed
	}
	return string(pt), nil
}

// Blind generates blind search tokens for a plaintext string.
// Returns a base64-encoded wire format string for the event meta.__blind field.
func (s *E2EESession) Blind(plaintext string) string {
	tokens := tokenize(plaintext)
	set := blindTokenSet{
		Words:    blindAll(s.veilKey[:], tokens.words),
		Trigrams: blindAll(s.veilKey[:], tokens.trigrams),
	}
	j, _ := json.Marshal(set)
	return base64.StdEncoding.EncodeToString(j)
}

// --- internal helpers ---

// hkdfSHA256 derives a 32-byte key using HKDF-SHA256 with empty salt.
func hkdfSHA256(ikm, info []byte) [32]byte {
	// Extract: PRK = HMAC-SHA256(salt=zeros(32), IKM)
	salt := make([]byte, 32)
	h := hmac.New(sha256.New, salt)
	h.Write(ikm)
	prk := h.Sum(nil)

	// Expand: OKM = HMAC-SHA256(PRK, info || 0x01)
	h = hmac.New(sha256.New, prk)
	h.Write(info)
	h.Write([]byte{0x01})
	okm := h.Sum(nil)

	var out [32]byte
	copy(out[:], okm)
	return out
}

// convergentNonce derives a deterministic 12-byte nonce from HMAC-SHA256(key, plaintext||aad).
func convergentNonce(key, plaintext, aad []byte) [nonceLen]byte {
	h := hmac.New(sha256.New, key)
	h.Write(plaintext)
	h.Write(aad)
	tag := h.Sum(nil)
	var nonce [nonceLen]byte
	copy(nonce[:], tag[:nonceLen])
	return nonce
}

// sealWithNonce encrypts and wraps in CiphertextEnvelope.
func (s *E2EESession) sealWithNonce(nonce [nonceLen]byte, plaintext, aad []byte) (string, error) {
	block, err := aes.NewCipher(s.cipherKey[:])
	if err != nil {
		return "", fmt.Errorf("herald: aes init: %w", err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return "", fmt.Errorf("herald: gcm init: %w", err)
	}

	// seal appends ciphertext+tag after nonce
	payload := make([]byte, nonceLen, nonceLen+len(plaintext)+tagLen)
	copy(payload, nonce[:])
	payload = gcm.Seal(payload, nonce[:], plaintext, aad)

	return encodeEnvelope(s.keyVersion, algoAES256GCM, payload), nil
}

// --- CiphertextEnvelope wire format ---

func encodeEnvelope(version uint32, algoID uint64, payload []byte) string {
	packed := (uint64(version) << 4) | algoID
	obfuscated := (packed * obfuskeyMult) % envelopeModulus
	prefix := encodeBase62(obfuscated)
	b64 := base64.RawURLEncoding.EncodeToString(payload)
	return prefix + ":" + b64
}

func decodeEnvelope(s string) (version uint32, payload []byte, err error) {
	colon := strings.IndexByte(s, ':')
	if colon < 0 {
		return 0, nil, ErrInvalidCiphertext
	}
	prefix := s[:colon]
	if len(prefix) == 0 {
		return 0, nil, ErrInvalidCiphertext
	}

	obfuscated, err := decodeBase62(prefix)
	if err != nil {
		return 0, nil, ErrInvalidCiphertext
	}
	packed := (obfuscated * obfuskeyInverse) % envelopeModulus
	version = uint32(packed >> 4)

	payload, err = base64.RawURLEncoding.DecodeString(s[colon+1:])
	if err != nil {
		return 0, nil, fmt.Errorf("herald: invalid base64url payload: %w", err)
	}
	return version, payload, nil
}

func encodeBase62(val uint64) string {
	if val == 0 {
		return strings.Repeat(string(envelopeAlphabet[0]), envelopeKeyLen)
	}
	var buf [envelopeKeyLen]byte
	for i := envelopeKeyLen - 1; i >= 0; i-- {
		buf[i] = envelopeAlphabet[val%envelopeBase]
		val /= envelopeBase
	}
	return string(buf[:])
}

func decodeBase62(s string) (uint64, error) {
	var val uint64
	for _, c := range s {
		if c > 255 {
			return 0, fmt.Errorf("invalid character %c", c)
		}
		idx := alphabetIndex[c]
		if idx < 0 {
			return 0, fmt.Errorf("invalid character %c", c)
		}
		val = val*envelopeBase + uint64(idx)
	}
	return val, nil
}

// --- Blind token generation ---

type tokenSet struct {
	words    []string
	trigrams []string
}

type blindTokenSet struct {
	Words    []string `json:"words"`
	Trigrams []string `json:"trigrams"`
}

func tokenize(text string) tokenSet {
	normalized := strings.ToLower(text)

	// Split on non-alphanumeric boundaries
	words := splitNonAlphanumeric(normalized)

	// Word tokens: "w:{word}"
	wordTokens := make([]string, 0, len(words))
	for _, w := range words {
		wordTokens = append(wordTokens, "w:"+w)
	}

	// Trigram tokens: "t:{3-char-window}" for words >= 3 chars
	var trigramTokens []string
	for _, w := range words {
		runes := []rune(w)
		if len(runes) >= 3 {
			for i := 0; i <= len(runes)-3; i++ {
				trigramTokens = append(trigramTokens, "t:"+string(runes[i:i+3]))
			}
		}
	}

	// Sort and dedup
	wordTokens = sortedDedup(wordTokens)
	trigramTokens = sortedDedup(trigramTokens)

	return tokenSet{words: wordTokens, trigrams: trigramTokens}
}

func splitNonAlphanumeric(s string) []string {
	var words []string
	var current []rune
	for _, r := range s {
		if unicode.IsLetter(r) || unicode.IsDigit(r) {
			current = append(current, r)
		} else {
			if len(current) > 0 {
				words = append(words, string(current))
				current = current[:0]
			}
		}
	}
	if len(current) > 0 {
		words = append(words, string(current))
	}
	return words
}

func sortedDedup(ss []string) []string {
	if len(ss) == 0 {
		return ss
	}
	sort.Strings(ss)
	j := 0
	for i := 1; i < len(ss); i++ {
		if ss[i] != ss[j] {
			j++
			ss[j] = ss[i]
		}
	}
	return ss[:j+1]
}

func blindAll(key []byte, tokens []string) []string {
	out := make([]string, len(tokens))
	for i, t := range tokens {
		h := hmac.New(sha256.New, key)
		h.Write([]byte(t))
		out[i] = fmt.Sprintf("%x", h.Sum(nil))
	}
	return out
}

// DecodeBlindTokens decodes a wire-format blind token string back to a BlindTokenSet.
func DecodeBlindTokens(wire string) (*blindTokenSet, error) {
	j, err := base64.StdEncoding.DecodeString(wire)
	if err != nil {
		return nil, fmt.Errorf("herald: invalid blind token wire format: %w", err)
	}
	var set blindTokenSet
	if err := json.Unmarshal(j, &set); err != nil {
		return nil, fmt.Errorf("herald: invalid blind token JSON: %w", err)
	}
	return &set, nil
}
