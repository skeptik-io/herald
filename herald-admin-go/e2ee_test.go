package herald

import (
	"encoding/hex"
	"encoding/json"
	"testing"
)

// Test vectors generated from the Rust shroudb-cipher-blind / shroudb-veil-blind crates.

func mustHex(s string) []byte {
	b, err := hex.DecodeString(s)
	if err != nil {
		panic(err)
	}
	return b
}

func to32(b []byte) [32]byte {
	var out [32]byte
	copy(out[:], b)
	return out
}

func TestHKDFCipherKey(t *testing.T) {
	shared := [32]byte{}
	for i := range shared {
		shared[i] = 0x42
	}
	key := hkdfSHA256(shared[:], []byte(hkdfCipherInfo))
	got := hex.EncodeToString(key[:])
	want := "0f1e00421174f25355b5f2608023e62ee0bd4928f89f4fb13cb22091f302e39a"
	if got != want {
		t.Fatalf("cipher key mismatch:\n  got  %s\n  want %s", got, want)
	}
}

func TestHKDFVeilKey(t *testing.T) {
	shared := [32]byte{}
	for i := range shared {
		shared[i] = 0x42
	}
	key := hkdfSHA256(shared[:], []byte(hkdfVeilInfo))
	got := hex.EncodeToString(key[:])
	want := "63f027ee28195cf6c2e35f568a8da9baa6b05145c50a7da27bce24c80547d3cf"
	if got != want {
		t.Fatalf("veil key mismatch:\n  got  %s\n  want %s", got, want)
	}
}

func TestX25519KeyExchange(t *testing.T) {
	aliceSecret := [32]byte{}
	for i := range aliceSecret {
		aliceSecret[i] = 1
	}
	bobSecret := [32]byte{}
	for i := range bobSecret {
		bobSecret[i] = 2
	}

	aliceKP := &E2EEKeyPair{SecretKey: aliceSecret}
	bobKP := &E2EEKeyPair{SecretKey: bobSecret}

	// Derive public keys via DeriveSharedSecret with basepoint (indirect test)
	aliceShared, err := DeriveSharedSecret(aliceKP.SecretKey, to32(mustHex("ce8d3ad1ccb633ec7b70c17814a5c76ecd029685050d344745ba05870e587d59")))
	if err != nil {
		t.Fatal(err)
	}
	bobShared, err := DeriveSharedSecret(bobKP.SecretKey, to32(mustHex("a4e09292b651c278b9772c569f5fa9bb13d906b46ab68c9df9dc2b4409f8a209")))
	if err != nil {
		t.Fatal(err)
	}

	if aliceShared != bobShared {
		t.Fatal("shared secrets don't match")
	}
	want := "2ed76ab549b1e73c031eb49c9448f0798aea81b698279a0c3dc3e49fbfc4b953"
	if hex.EncodeToString(aliceShared[:]) != want {
		t.Fatalf("shared secret mismatch:\n  got  %s\n  want %s", hex.EncodeToString(aliceShared[:]), want)
	}
}

func TestConvergentEncryption(t *testing.T) {
	shared := [32]byte{}
	for i := range shared {
		shared[i] = 0x42
	}
	session := CreateSession(shared, 1)

	ct, err := session.EncryptConvergent("hello world", "stream-123")
	if err != nil {
		t.Fatal(err)
	}

	want := "QRWp:mL-5BlPGudFhQ7Oiga-OEWKXny6bQ9UGlzmWeqXNSUnLfW62LbO1"
	if ct != want {
		t.Fatalf("convergent ciphertext mismatch:\n  got  %s\n  want %s", ct, want)
	}

	// Decrypt
	pt, err := session.Decrypt(ct, "stream-123")
	if err != nil {
		t.Fatal(err)
	}
	if pt != "hello world" {
		t.Fatalf("decrypted text mismatch: got %q want %q", pt, "hello world")
	}
}

func TestRandomEncryptDecrypt(t *testing.T) {
	shared := [32]byte{}
	for i := range shared {
		shared[i] = 0x42
	}
	session := CreateSession(shared, 1)

	ct, err := session.Encrypt("hello world", "stream-123")
	if err != nil {
		t.Fatal(err)
	}

	// Must start with same prefix
	if ct[:4] != "QRWp" {
		t.Fatalf("prefix mismatch: got %q want QRWp", ct[:4])
	}

	pt, err := session.Decrypt(ct, "stream-123")
	if err != nil {
		t.Fatal(err)
	}
	if pt != "hello world" {
		t.Fatalf("decrypted text mismatch: got %q want %q", pt, "hello world")
	}
}

func TestDecryptWrongAAD(t *testing.T) {
	shared := [32]byte{}
	for i := range shared {
		shared[i] = 0x42
	}
	session := CreateSession(shared, 1)

	ct, err := session.Encrypt("hello world", "stream-123")
	if err != nil {
		t.Fatal(err)
	}

	_, err = session.Decrypt(ct, "wrong-stream")
	if err == nil {
		t.Fatal("expected decryption to fail with wrong AAD")
	}
}

func TestBlindTokens(t *testing.T) {
	shared := [32]byte{}
	for i := range shared {
		shared[i] = 0x42
	}
	session := CreateSession(shared, 1)

	wire := session.Blind("Hello World")

	// Decode and verify
	set, err := DecodeBlindTokens(wire)
	if err != nil {
		t.Fatal(err)
	}

	wantWords := []string{
		"64927ad8770952a8f4eb5c01e9365d9c29f1de103516007ed26a0aa38f4c0bc4",
		"33fd718299d726735b90361b89bf131b624e83d5e30165f888a50b220c5c0e53",
	}
	wantTrigrams := []string{
		"159950c56432b88c3d1e98fd1b41a86dd43de392327637758a3b713830b191bf",
		"1f7ddfd1f36053b7646f4bb5e0acf4b8fbd5a741f0565839a4def87d23f7d33e",
		"dd27dab20b0561256c2795345d2a78e1997dbf539f24f0879571f65fdf3058d2",
		"bcf5d64bc6c25f53fbbef004bd668f4615007c446cf23657c401e63a0c39b2ff",
		"681173c3117c9e51782ba884c5fc5d9dd8b0893ec70d5f29aeef12c88004611b",
		"5e69b89485f166cb35dcae314549dad5a7aa88b4b2aaab891fd274eb4f684967",
	}

	if len(set.Words) != len(wantWords) {
		t.Fatalf("word count mismatch: got %d want %d", len(set.Words), len(wantWords))
	}
	for i, w := range wantWords {
		if set.Words[i] != w {
			t.Fatalf("word[%d] mismatch:\n  got  %s\n  want %s", i, set.Words[i], w)
		}
	}
	if len(set.Trigrams) != len(wantTrigrams) {
		t.Fatalf("trigram count mismatch: got %d want %d", len(set.Trigrams), len(wantTrigrams))
	}
	for i, tr := range wantTrigrams {
		if set.Trigrams[i] != tr {
			t.Fatalf("trigram[%d] mismatch:\n  got  %s\n  want %s", i, set.Trigrams[i], tr)
		}
	}
}

func TestBlindTokenWireFormat(t *testing.T) {
	shared := [32]byte{}
	for i := range shared {
		shared[i] = 0x42
	}
	session := CreateSession(shared, 1)

	wire := session.Blind("Hello World")

	// Verify it's valid base64 → valid JSON
	set, err := DecodeBlindTokens(wire)
	if err != nil {
		t.Fatal(err)
	}

	// Re-encode and check structure
	j, _ := json.Marshal(set)
	var m map[string]interface{}
	if err := json.Unmarshal(j, &m); err != nil {
		t.Fatal(err)
	}
	if _, ok := m["words"]; !ok {
		t.Fatal("missing 'words' key")
	}
	if _, ok := m["trigrams"]; !ok {
		t.Fatal("missing 'trigrams' key")
	}
}

func TestEnvelopePrefixRoundtrip(t *testing.T) {
	tests := []struct {
		version uint32
		algoID  uint64
		prefix  string
	}{
		{0, 0, "kkkk"},
		{1, 0, "QRWp"},
		{2, 0, "dv98"},
		{3, 0, "rpgd"},
		{0, 1, "W0E3"},
		{1, 1, "oChD"},
	}

	for _, tt := range tests {
		packed := (uint64(tt.version) << 4) | tt.algoID
		obfuscated := (packed * obfuskeyMult) % envelopeModulus
		got := encodeBase62(obfuscated)
		if got != tt.prefix {
			t.Errorf("encode v=%d a=%d: got %q want %q", tt.version, tt.algoID, got, tt.prefix)
		}

		// Roundtrip
		decoded, err := decodeBase62(got)
		if err != nil {
			t.Fatal(err)
		}
		decodedPacked := (decoded * obfuskeyInverse) % envelopeModulus
		gotV := uint32(decodedPacked >> 4)
		gotA := decodedPacked & 0xF
		if gotV != tt.version || gotA != tt.algoID {
			t.Errorf("decode %q: got v=%d a=%d want v=%d a=%d", got, gotV, gotA, tt.version, tt.algoID)
		}
	}
}

func TestSessionExportRestore(t *testing.T) {
	shared := [32]byte{}
	for i := range shared {
		shared[i] = 0x42
	}
	session := CreateSession(shared, 1)

	ct, err := session.EncryptConvergent("test message", "room-1")
	if err != nil {
		t.Fatal(err)
	}

	// Export and restore
	restored := RestoreSession(session.ExportCipherKey(), session.ExportVeilKey(), 1)

	pt, err := restored.Decrypt(ct, "room-1")
	if err != nil {
		t.Fatal(err)
	}
	if pt != "test message" {
		t.Fatalf("restored session decrypt mismatch: got %q", pt)
	}

	// Blind tokens should match
	wire1 := session.Blind("test")
	wire2 := restored.Blind("test")
	if wire1 != wire2 {
		t.Fatal("restored session blind tokens don't match")
	}
}

func TestTokenize(t *testing.T) {
	ts := tokenize("Hello World")

	wantWords := []string{"w:hello", "w:world"}
	wantTrigrams := []string{"t:ell", "t:hel", "t:llo", "t:orl", "t:rld", "t:wor"}

	if len(ts.words) != len(wantWords) {
		t.Fatalf("word count: got %d want %d", len(ts.words), len(wantWords))
	}
	for i, w := range wantWords {
		if ts.words[i] != w {
			t.Errorf("word[%d]: got %q want %q", i, ts.words[i], w)
		}
	}
	if len(ts.trigrams) != len(wantTrigrams) {
		t.Fatalf("trigram count: got %d want %d", len(ts.trigrams), len(wantTrigrams))
	}
	for i, tr := range wantTrigrams {
		if ts.trigrams[i] != tr {
			t.Errorf("trigram[%d]: got %q want %q", i, ts.trigrams[i], tr)
		}
	}
}
