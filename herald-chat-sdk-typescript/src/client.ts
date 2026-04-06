import { type HeraldClient, nextRef, type EventAck } from "herald-sdk";

/**
 * Herald Chat WebSocket client — adds conversational frame methods
 * (edit, delete, cursors, presence, typing, reactions) on top of HeraldClient.
 */
export class HeraldChatClient {
  constructor(private client: HeraldClient) {}

  async editEvent(stream: string, id: string, body: string): Promise<EventAck> {
    const ref = nextRef();
    const e2ee = this.client.e2eeManager;
    const { body: finalBody } = e2ee
      ? e2ee.encryptOutgoing(stream, body)
      : { body };
    return this.client.requestFrame(ref, {
      type: "event.edit",
      ref,
      payload: { stream, id, body: finalBody },
    }) as Promise<EventAck>;
  }

  async deleteEvent(stream: string, id: string): Promise<EventAck> {
    const ref = nextRef();
    return this.client.requestFrame(ref, {
      type: "event.delete",
      ref,
      payload: { stream, id },
    }) as Promise<EventAck>;
  }

  updateCursor(stream: string, seq: number): void {
    this.client.sendFrame({
      type: "cursor.update",
      payload: { stream, seq },
    });
  }

  setPresence(status: "online" | "away" | "dnd"): void {
    this.client.sendFrame({
      type: "presence.set",
      payload: { status },
    });
  }

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

  addReaction(stream: string, eventId: string, emoji: string): void {
    this.client.sendFrame({
      type: "reaction.add",
      payload: { stream, event_id: eventId, emoji },
    });
  }

  removeReaction(stream: string, eventId: string, emoji: string): void {
    this.client.sendFrame({
      type: "reaction.remove",
      payload: { stream, event_id: eventId, emoji },
    });
  }
}
