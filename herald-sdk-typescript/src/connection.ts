import type { ServerFrame } from "./types.js";

export type OnFrameCallback = (frame: ServerFrame) => void;
export type OnStateCallback = (state: ConnectionState) => void;

export type ConnectionState = "connecting" | "connected" | "disconnected" | "reconnecting";

const MAX_RECONNECT_DELAY = 30_000;
const BASE_RECONNECT_DELAY = 1_000;

export class Connection {
  private ws: WebSocket | null = null;
  private reconnectAttempt = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private intentionalClose = false;

  private state: ConnectionState = "disconnected";
  private onFrame: OnFrameCallback;
  private onStateChange: OnStateCallback;

  constructor(
    private url: string,
    private reconnectEnabled: boolean,
    private maxDelay: number,
    onFrame: OnFrameCallback,
    onStateChange: OnStateCallback,
  ) {
    this.onFrame = onFrame;
    this.onStateChange = onStateChange;
    this.maxDelay = maxDelay || MAX_RECONNECT_DELAY;
  }

  connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      this.intentionalClose = false;
      this.setState("connecting");

      const ws = new WebSocket(this.url);

      ws.onopen = () => {
        this.ws = ws;
        this.reconnectAttempt = 0;
        this.setState("connected");
        resolve();
      };

      ws.onmessage = (event) => {
        try {
          const frame = JSON.parse(event.data as string) as ServerFrame;
          this.onFrame(frame);
        } catch {
          // Ignore malformed frames
        }
      };

      ws.onclose = () => {
        this.ws = null;
        if (this.intentionalClose) {
          this.setState("disconnected");
          return;
        }
        this.setState("disconnected");
        if (this.reconnectEnabled) {
          this.scheduleReconnect();
        }
      };

      ws.onerror = () => {
        if (this.state === "connecting") {
          reject(new Error("WebSocket connection failed"));
        }
      };
    });
  }

  send(frame: Record<string, unknown>): void {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(frame));
    }
  }

  close(): void {
    this.intentionalClose = true;
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
    this.setState("disconnected");
  }

  get isConnected(): boolean {
    return this.state === "connected";
  }

  private setState(state: ConnectionState): void {
    if (this.state !== state) {
      this.state = state;
      this.onStateChange(state);
    }
  }

  private scheduleReconnect(): void {
    this.setState("reconnecting");
    const delay = Math.min(
      BASE_RECONNECT_DELAY * 2 ** this.reconnectAttempt,
      this.maxDelay,
    );
    // Add jitter: 0.5x to 1.5x
    const jitter = delay * (0.5 + Math.random());
    this.reconnectAttempt++;

    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect().catch(() => {
        // Will trigger onclose → scheduleReconnect again
      });
    }, jitter);
  }
}
