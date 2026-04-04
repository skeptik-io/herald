#!/usr/bin/env bash
set -euo pipefail

# ring requires a WASM-capable C compiler (not Apple's clang).
# Homebrew LLVM has wasm32 backend support.
export CC_wasm32_unknown_unknown="${CC_wasm32_unknown_unknown:-/opt/homebrew/opt/llvm/bin/clang}"
export AR_wasm32_unknown_unknown="${AR_wasm32_unknown_unknown:-/opt/homebrew/opt/llvm/bin/llvm-ar}"

# Use rustup's toolchain (not Homebrew's) for wasm32 target support.
RUSTUP_BIN="${HOME}/.rustup/toolchains/stable-aarch64-apple-darwin/bin"
if [ -d "$RUSTUP_BIN" ]; then
  export PATH="${RUSTUP_BIN}:${PATH}"
  export RUSTC="${RUSTUP_BIN}/rustc"
fi

wasm-pack build --target web --out-dir ../src/wasm-pkg --release
