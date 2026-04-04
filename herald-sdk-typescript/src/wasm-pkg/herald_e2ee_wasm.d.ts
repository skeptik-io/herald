/* tslint:disable */
/* eslint-disable */

/**
 * Opaque handle holding both cipher and veil keys for a session.
 *
 * A session is created from a shared secret (after x25519 key exchange)
 * or restored from previously exported key bytes.
 */
export class Session {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Generate blind tokens for a plaintext string.
     *
     * Returns a base64-encoded wire format string suitable for
     * storing in the message `meta.__blind` field.
     */
    blind(plaintext: string): string;
    /**
     * Decrypt a CiphertextEnvelope string. The room ID must match the
     * value used during encryption.
     *
     * Returns the plaintext string.
     */
    decrypt(ciphertext: string, room_id: string): string;
    /**
     * Encrypt a plaintext string. The room ID is used as AAD (Additional
     * Authenticated Data), binding the ciphertext to the room context.
     *
     * Returns a CiphertextEnvelope-formatted string.
     */
    encrypt(plaintext: string, room_id: string): string;
    /**
     * Export cipher key bytes for persistence.
     */
    exportCipherKey(): Uint8Array;
    /**
     * Export veil key bytes for persistence.
     */
    exportVeilKey(): Uint8Array;
}

/**
 * Create a new E2EE session from a shared secret.
 *
 * Derives both cipher and veil keys from the shared secret using
 * different HKDF domain separators.
 */
export function createSession(shared_secret: Uint8Array, version: number): Session;

/**
 * Compute a shared secret from our secret key and their public key.
 */
export function deriveSharedSecret(my_secret: Uint8Array, their_public: Uint8Array): Uint8Array;

/**
 * Generate a new x25519 keypair for E2EE key exchange.
 *
 * Returns `{ publicKey: Uint8Array, secretKey: Uint8Array }`.
 */
export function generateKeyPair(): any;

/**
 * Restore a session from previously exported key bytes.
 */
export function restoreSession(cipher_key_bytes: Uint8Array, veil_key_bytes: Uint8Array, version: number): Session;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_session_free: (a: number, b: number) => void;
    readonly createSession: (a: number, b: number, c: number) => [number, number, number];
    readonly deriveSharedSecret: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly generateKeyPair: () => [number, number, number];
    readonly restoreSession: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
    readonly session_blind: (a: number, b: number, c: number) => [number, number, number, number];
    readonly session_decrypt: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly session_encrypt: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly session_exportCipherKey: (a: number) => [number, number];
    readonly session_exportVeilKey: (a: number) => [number, number];
    readonly ring_core_0_17_14__bn_mul_mont: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
