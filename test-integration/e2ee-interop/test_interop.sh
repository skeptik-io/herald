#!/usr/bin/env bash
set -euo pipefail

# Cross-SDK E2EE interop test
# Verifies that Go, Python, and Ruby E2EE modules produce identical output
# and can decrypt each other's ciphertexts.

cd "$(dirname "$0")"

SHARED_SECRET="4242424242424242424242424242424242424242424242424242424242424242"
VERSION=1
PLAINTEXT="Hello from cross-SDK interop test!"
STREAM_ID="interop-stream-001"
BLIND_TEXT="Hello World"

passed=0
failed=0

pass() { passed=$((passed + 1)); echo "  ok - $1"; }
fail() { failed=$((failed + 1)); echo "  FAIL - $1"; }

check() {
  local name="$1" got="$2" want="$3"
  if [ "$got" = "$want" ]; then
    pass "$name"
  else
    fail "$name: got '$got', want '$want'"
  fi
}

echo "Building Go CLI..."
GO_BIN=$(mktemp)
(cd ../../herald-admin-go && go build -o "$GO_BIN" ../test-integration/e2ee-interop/go_e2ee_cli.go) 2>&1
trap "rm -f $GO_BIN" EXIT

echo ""
echo "=== E2EE Cross-SDK Interop Tests ==="
echo ""

# --- Convergent encryption: each SDK should produce the same ciphertext ---
echo "# Convergent encryption agreement"

GO_CT=$("$GO_BIN" encrypt "$SHARED_SECRET" "$VERSION" "$PLAINTEXT" "$STREAM_ID" | python3 -c "import sys,json; print(json.load(sys.stdin)['ciphertext'])")
PY_CT=$(python3 py_e2ee_cli.py encrypt "$SHARED_SECRET" "$VERSION" "$PLAINTEXT" "$STREAM_ID" | python3 -c "import sys,json; print(json.load(sys.stdin)['ciphertext'])")
RB_CT=$(ruby rb_e2ee_cli.rb encrypt "$SHARED_SECRET" "$VERSION" "$PLAINTEXT" "$STREAM_ID" | python3 -c "import sys,json; print(json.load(sys.stdin)['ciphertext'])")

check "Go convergent encryption matches Python" "$GO_CT" "$PY_CT"
check "Go convergent encryption matches Ruby" "$GO_CT" "$RB_CT"
check "Python convergent encryption matches Ruby" "$PY_CT" "$RB_CT"

echo ""
echo "# Cross-SDK decrypt (each SDK decrypts ciphertext from the others)"

# Go encrypts → Python decrypts
PY_DEC=$(python3 py_e2ee_cli.py decrypt "$SHARED_SECRET" "$VERSION" "$GO_CT" "$STREAM_ID" | python3 -c "import sys,json; print(json.load(sys.stdin)['plaintext'])")
check "Go encrypt → Python decrypt" "$PY_DEC" "$PLAINTEXT"

# Go encrypts → Ruby decrypts
RB_DEC=$(ruby rb_e2ee_cli.rb decrypt "$SHARED_SECRET" "$VERSION" "$GO_CT" "$STREAM_ID" | python3 -c "import sys,json; print(json.load(sys.stdin)['plaintext'])")
check "Go encrypt → Ruby decrypt" "$RB_DEC" "$PLAINTEXT"

# Python encrypts → Go decrypts
GO_DEC=$("$GO_BIN" decrypt "$SHARED_SECRET" "$VERSION" "$PY_CT" "$STREAM_ID" | python3 -c "import sys,json; print(json.load(sys.stdin)['plaintext'])")
check "Python encrypt → Go decrypt" "$GO_DEC" "$PLAINTEXT"

# Python encrypts → Ruby decrypts
RB_DEC2=$(ruby rb_e2ee_cli.rb decrypt "$SHARED_SECRET" "$VERSION" "$PY_CT" "$STREAM_ID" | python3 -c "import sys,json; print(json.load(sys.stdin)['plaintext'])")
check "Python encrypt → Ruby decrypt" "$RB_DEC2" "$PLAINTEXT"

# Ruby encrypts → Go decrypts
GO_DEC2=$("$GO_BIN" decrypt "$SHARED_SECRET" "$VERSION" "$RB_CT" "$STREAM_ID" | python3 -c "import sys,json; print(json.load(sys.stdin)['plaintext'])")
check "Ruby encrypt → Go decrypt" "$GO_DEC2" "$PLAINTEXT"

# Ruby encrypts → Python decrypts
PY_DEC2=$(python3 py_e2ee_cli.py decrypt "$SHARED_SECRET" "$VERSION" "$RB_CT" "$STREAM_ID" | python3 -c "import sys,json; print(json.load(sys.stdin)['plaintext'])")
check "Ruby encrypt → Python decrypt" "$PY_DEC2" "$PLAINTEXT"

echo ""
echo "# Blind token agreement across SDKs"

GO_BLIND=$("$GO_BIN" blind "$SHARED_SECRET" "$BLIND_TEXT" | python3 -c "import sys,json; print(json.load(sys.stdin)['wire'])")
PY_BLIND=$(python3 py_e2ee_cli.py blind "$SHARED_SECRET" "$BLIND_TEXT" | python3 -c "import sys,json; print(json.load(sys.stdin)['wire'])")
RB_BLIND=$(ruby rb_e2ee_cli.rb blind "$SHARED_SECRET" "$BLIND_TEXT" | python3 -c "import sys,json; print(json.load(sys.stdin)['wire'])")

# Decode and compare the token sets (JSON key order may vary, so compare decoded)
compare_blind() {
  local name="$1" a="$2" b="$3"
  local match
  match=$(python3 -c "
import base64, json, sys
a = json.loads(base64.b64decode('$a'))
b = json.loads(base64.b64decode('$b'))
print('true' if a == b else 'false')
")
  check "$name" "$match" "true"
}

compare_blind "Go blind tokens match Python" "$GO_BLIND" "$PY_BLIND"
compare_blind "Go blind tokens match Ruby" "$GO_BLIND" "$RB_BLIND"
compare_blind "Python blind tokens match Ruby" "$PY_BLIND" "$RB_BLIND"

echo ""
echo "# Verify Rust reference test vector"

RUST_CT="QRWp:mL-5BlPGudFhQ7Oiga-OEWKXny6bQ9UGlzmWeqXNSUnLfW62LbO1"
RUST_SECRET="4242424242424242424242424242424242424242424242424242424242424242"

GO_DEC3=$("$GO_BIN" decrypt "$RUST_SECRET" 1 "$RUST_CT" "stream-123" | python3 -c "import sys,json; print(json.load(sys.stdin)['plaintext'])")
check "Go decrypts Rust reference ciphertext" "$GO_DEC3" "hello world"

PY_DEC3=$(python3 py_e2ee_cli.py decrypt "$RUST_SECRET" 1 "$RUST_CT" "stream-123" | python3 -c "import sys,json; print(json.load(sys.stdin)['plaintext'])")
check "Python decrypts Rust reference ciphertext" "$PY_DEC3" "hello world"

RB_DEC3=$(ruby rb_e2ee_cli.rb decrypt "$RUST_SECRET" 1 "$RUST_CT" "stream-123" | python3 -c "import sys,json; print(json.load(sys.stdin)['plaintext'])")
check "Ruby decrypts Rust reference ciphertext" "$RB_DEC3" "hello world"

echo ""
echo "=================================================="
echo "e2ee-interop: $passed passed, $failed failed"
if [ "$failed" -gt 0 ]; then exit 1; fi
