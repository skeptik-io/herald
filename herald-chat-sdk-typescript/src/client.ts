import type { HeraldClient } from "herald-sdk";

/**
 * Herald Chat WebSocket client — typing-indicator frame helpers.
 *
 * All other chat-state mutations (edits, deletes, reactions, cursor
 * advances) now ride the standard `event.publish` path as typed
 * envelopes. See `herald-chat-core`'s envelope dispatcher for the
 * receive-side state machine.
 */
export class HeraldChatClient {
  constructor(private client: HeraldClient) {}

  startTyping(stream: string): void {
    this.client.sendFrame({
      type: "typing.start",
      payload: { stream },
    });
  }

  stopTyping(stream: string): void {
    this.client.sendFrame({
      type: "typing.stop",
      payload: { stream },
    });
  }
}
