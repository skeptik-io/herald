/**
 * E2EE primitives powered by WASM (shroudb-cipher-blind + shroudb-veil-blind).
 *
 * The WASM module is lazy-loaded — non-E2EE users pay zero cost.
 */

import type { Session as WasmSession } from "./wasm-pkg/herald_e2ee_wasm.js";

export interface E2EEKeyPair {
  publicKey: Uint8Array;
  secretKey: Uint8Array;
}

export interface E2EESession {
  /** Encrypt plaintext with room ID as AAD. Returns CiphertextEnvelope string. */
  encrypt(plaintext: string, roomId: string): string;
  /** Decrypt CiphertextEnvelope string. Room ID must match encryption. */
  decrypt(ciphertext: string, roomId: string): string;
  /** Generate blind tokens for searchable encryption. Returns base64 wire format. */
  blind(plaintext: string): string;
  /** Export keys for persistence (e.g. IndexedDB). */
  exportKeys(): { cipherKey: Uint8Array; veilKey: Uint8Array };
  /** Free WASM memory. Called automatically via FinalizationRegistry. */
  destroy(): void;
}

// Lazy WASM module reference
let wasmModule: typeof import("./wasm-pkg/herald_e2ee_wasm.js") | null = null;

// Prevent double-initialization
let initPromise: Promise<void> | null = null;

// Automatic cleanup of WASM handles
const registry =
  typeof FinalizationRegistry !== "undefined"
    ? new FinalizationRegistry<WasmSession>((session) => session.free())
    : null;

function ensureInit(): typeof import("./wasm-pkg/herald_e2ee_wasm.js") {
  if (!wasmModule) {
    throw new Error("E2EE not initialized. Call initE2EE() first.");
  }
  return wasmModule;
}

function wrapSession(handle: WasmSession): E2EESession {
  const session: E2EESession = {
    encrypt: (plaintext, roomId) => handle.encrypt(plaintext, roomId),
    decrypt: (ciphertext, roomId) => handle.decrypt(ciphertext, roomId),
    blind: (plaintext) => handle.blind(plaintext),
    exportKeys: () => ({
      cipherKey: handle.exportCipherKey(),
      veilKey: handle.exportVeilKey(),
    }),
    destroy: () => handle.free(),
  };
  registry?.register(session, handle);
  return session;
}

/**
 * Initialize the E2EE WASM module. Must be called before any other E2EE
 * functions. Safe to call multiple times — subsequent calls are no-ops.
 */
export async function initE2EE(): Promise<void> {
  if (wasmModule) return;
  if (initPromise) return initPromise;
  initPromise = (async () => {
    const mod = await import("./wasm-pkg/herald_e2ee_wasm.js");
    await mod.default();
    wasmModule = mod;
  })();
  return initPromise;
}

/** Generate a new x25519 keypair for key exchange. */
export function generateKeyPair(): E2EEKeyPair {
  const wasm = ensureInit();
  return wasm.generateKeyPair() as E2EEKeyPair;
}

/** Compute shared secret from our secret key and their public key. */
export function deriveSharedSecret(
  mySecret: Uint8Array,
  theirPublic: Uint8Array,
): Uint8Array {
  const wasm = ensureInit();
  return wasm.deriveSharedSecret(mySecret, theirPublic);
}

/**
 * Create an E2EE session from a shared secret (after x25519 exchange).
 * Derives both cipher and veil keys with different domain separators.
 */
export function createSession(
  sharedSecret: Uint8Array,
  version = 1,
): E2EESession {
  const wasm = ensureInit();
  return wrapSession(wasm.createSession(sharedSecret, version));
}

/** Restore a session from previously exported key bytes. */
export function restoreSession(
  cipherKey: Uint8Array,
  veilKey: Uint8Array,
  version = 1,
): E2EESession {
  const wasm = ensureInit();
  return wrapSession(wasm.restoreSession(cipherKey, veilKey, version));
}
