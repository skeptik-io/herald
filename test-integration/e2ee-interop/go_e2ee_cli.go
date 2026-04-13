// +build ignore

// Small CLI for E2EE cross-SDK interop testing.
// Usage:
//   go run go_e2ee_cli.go encrypt <hex_shared_secret> <version> <plaintext> <stream_id>
//   go run go_e2ee_cli.go decrypt <hex_shared_secret> <version> <ciphertext> <stream_id>
//   go run go_e2ee_cli.go blind   <hex_shared_secret> <plaintext>
//
// Outputs JSON to stdout.

package main

import (
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"strconv"

	herald "github.com/skeptik-io/herald-admin-go"
)

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintln(os.Stderr, "usage: go_e2ee_cli <encrypt|decrypt|blind> ...")
		os.Exit(1)
	}
	cmd := os.Args[1]

	switch cmd {
	case "encrypt":
		if len(os.Args) != 6 {
			fmt.Fprintln(os.Stderr, "usage: encrypt <hex_secret> <version> <plaintext> <stream_id>")
			os.Exit(1)
		}
		secret := mustDecodeHex(os.Args[2])
		version := mustParseUint(os.Args[3])
		plaintext := os.Args[4]
		streamID := os.Args[5]

		session := herald.CreateSession(to32(secret), uint32(version))
		ct, err := session.EncryptConvergent(plaintext, streamID)
		if err != nil {
			fmt.Fprintf(os.Stderr, "encrypt error: %v\n", err)
			os.Exit(1)
		}
		out, _ := json.Marshal(map[string]string{"ciphertext": ct})
		fmt.Println(string(out))

	case "decrypt":
		if len(os.Args) != 6 {
			fmt.Fprintln(os.Stderr, "usage: decrypt <hex_secret> <version> <ciphertext> <stream_id>")
			os.Exit(1)
		}
		secret := mustDecodeHex(os.Args[2])
		version := mustParseUint(os.Args[3])
		ciphertext := os.Args[4]
		streamID := os.Args[5]

		session := herald.CreateSession(to32(secret), uint32(version))
		pt, err := session.Decrypt(ciphertext, streamID)
		if err != nil {
			fmt.Fprintf(os.Stderr, "decrypt error: %v\n", err)
			os.Exit(1)
		}
		out, _ := json.Marshal(map[string]string{"plaintext": pt})
		fmt.Println(string(out))

	case "blind":
		if len(os.Args) != 4 {
			fmt.Fprintln(os.Stderr, "usage: blind <hex_secret> <plaintext>")
			os.Exit(1)
		}
		secret := mustDecodeHex(os.Args[2])
		plaintext := os.Args[3]

		session := herald.CreateSession(to32(secret), 1)
		wire := session.Blind(plaintext)
		out, _ := json.Marshal(map[string]string{"wire": wire})
		fmt.Println(string(out))

	default:
		fmt.Fprintf(os.Stderr, "unknown command: %s\n", cmd)
		os.Exit(1)
	}
}

func mustDecodeHex(s string) []byte {
	b, err := hex.DecodeString(s)
	if err != nil {
		fmt.Fprintf(os.Stderr, "invalid hex: %v\n", err)
		os.Exit(1)
	}
	return b
}

func mustParseUint(s string) uint64 {
	n, err := strconv.ParseUint(s, 10, 32)
	if err != nil {
		fmt.Fprintf(os.Stderr, "invalid uint: %v\n", err)
		os.Exit(1)
	}
	return n
}

func to32(b []byte) [32]byte {
	var out [32]byte
	copy(out[:], b)
	return out
}
