/**
 * Chat envelope shapes carried in `EventNew.body`.
 *
 * Herald is a pure transport — the server treats `body` as opaque bytes and
 * does not interpret the envelope. Chat semantics (reactions, edits, deletes,
 * cursor advancements) ride the same `event.publish` / `event.new` path as
 * messages, distinguished only by the `kind` discriminant inside this
 * envelope. Chat-core parses the envelope on receipt and dispatches to the
 * appropriate store mutation.
 *
 * Why `body` and not `meta`:
 *   - `body` is already opaque + encryptable end-to-end via the SDK's E2EE
 *     manager, so envelope contents (including emoji, target IDs, edit text)
 *     can be encrypted without special-casing.
 *   - `meta` is reserved for application-defined transport metadata and may
 *     be subject to structural validation in some paths.
 *
 * Transport-level fields (`parent_id`, `meta`) stay on `EventNew` and are
 * not duplicated inside the envelope.
 */

export type ChatEnvelope =
  | MessageEnvelope
  | ReactionEnvelope
  | EditEnvelope
  | DeleteEnvelope
  | CursorEnvelope;

export interface MessageEnvelope {
  kind: "message";
  /** The message text. */
  text: string;
}

export interface ReactionEnvelope {
  kind: "reaction";
  /** Event ID being reacted to. */
  targetId: string;
  op: "add" | "remove";
  emoji: string;
}

export interface EditEnvelope {
  kind: "edit";
  /** Event ID being edited. */
  targetId: string;
  /** New body text. */
  text: string;
}

export interface DeleteEnvelope {
  kind: "delete";
  /** Event ID being deleted. */
  targetId: string;
}

export interface CursorEnvelope {
  kind: "cursor";
  /** Read-cursor high-water-mark (server seq). */
  seq: number;
}

const KINDS = new Set(["message", "reaction", "edit", "delete", "cursor"]);

/** JSON-encode an envelope for placement in `EventPublish.body`. */
export function encodeEnvelope(envelope: ChatEnvelope): string {
  return JSON.stringify(envelope);
}

/**
 * Parse an `EventNew.body` as a chat envelope. Returns `null` when the body
 * is not a recognized envelope — callers should treat such bodies as plain
 * message text for backward compatibility with non-enveloped publishers.
 *
 * The parser is intentionally strict: it requires a JSON object whose `kind`
 * field is one of the known discriminants. Unknown kinds return `null` so
 * future envelope additions don't blow up on older clients.
 */
export function parseEnvelope(body: string): ChatEnvelope | null {
  if (!body) return null;
  // Fast reject: a JSON object body always starts with `{`. Plain message
  // text rarely does. This avoids JSON.parse on every event.
  if (body.charCodeAt(0) !== 123 /* { */) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(body);
  } catch {
    return null;
  }
  if (!parsed || typeof parsed !== "object") return null;
  const kind = (parsed as Record<string, unknown>).kind;
  if (typeof kind !== "string" || !KINDS.has(kind)) return null;
  return parsed as ChatEnvelope;
}
