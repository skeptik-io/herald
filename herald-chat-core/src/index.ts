export { ChatCore } from "./chat-core.js";
export { Notifier } from "./notifier.js";
export { LivenessController, browserEnvironment } from "./liveness/liveness.js";
export { encodeEnvelope, parseEnvelope } from "./envelope.js";

export type {
  Message,
  MessageStatus,
  PendingMessage,
  Member,
  ScrollStateSnapshot,
  LivenessState,
  LivenessConfig,
  LivenessEnvironment,
  ChatCoreOptions,
  ChatEvent,
  ChatWriter,
  MessageDraft,
  MessageAck,
  Middleware,
  PresenceStatus,
} from "./types.js";

export type {
  ChatEnvelope,
  MessageEnvelope,
  ReactionEnvelope,
  EditEnvelope,
  DeleteEnvelope,
  CursorEnvelope,
} from "./envelope.js";

// Re-export SDK types consumers need for seedHistory / loadMoreWith
export type { EventNew } from "herald-sdk";
