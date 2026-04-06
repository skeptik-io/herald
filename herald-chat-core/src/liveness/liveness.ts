import type { LivenessState, LivenessConfig, LivenessEnvironment } from "../types.js";

const ACTIVITY_EVENTS = ["mousemove", "mousedown", "keydown", "touchstart", "scroll"];

export class LivenessController {
  private state: LivenessState = "active";
  private idleTimer: number | null = null;
  private lastActivity = 0;
  private env: LivenessEnvironment;
  private idleTimeoutMs: number;
  private throttleMs: number;
  private onChange: (state: LivenessState) => void;
  private attached = false;

  // Bound handlers for clean detach
  private boundActivity: () => void;
  private boundVisibility: () => void;

  constructor(
    env: LivenessEnvironment,
    config: LivenessConfig,
    onChange: (state: LivenessState) => void,
  ) {
    this.env = env;
    this.idleTimeoutMs = config.idleTimeoutMs ?? 120_000;
    this.throttleMs = config.throttleMs ?? 5_000;
    this.onChange = onChange;
    this.boundActivity = () => this.handleActivity();
    this.boundVisibility = () => this.handleVisibilityChange();
  }

  getState(): LivenessState {
    return this.state;
  }

  attach(): void {
    if (this.attached) return;
    this.attached = true;
    this.lastActivity = Date.now();

    for (const event of ACTIVITY_EVENTS) {
      this.env.addEventListener("document", event, this.boundActivity);
    }
    this.env.addEventListener("document", "visibilitychange", this.boundVisibility);

    this.resetIdleTimer();
  }

  detach(): void {
    if (!this.attached) return;
    this.attached = false;

    for (const event of ACTIVITY_EVENTS) {
      this.env.removeEventListener("document", event, this.boundActivity);
    }
    this.env.removeEventListener("document", "visibilitychange", this.boundVisibility);

    if (this.idleTimer !== null) {
      this.env.clearTimeout(this.idleTimer);
      this.idleTimer = null;
    }
  }

  private handleActivity(): void {
    const now = Date.now();
    if (now - this.lastActivity < this.throttleMs) return;
    this.lastActivity = now;
    this.resetIdleTimer();

    if (this.state === "idle") {
      this.transition("active");
    }
  }

  private handleVisibilityChange(): void {
    const vis = this.env.getVisibilityState();
    if (vis === "hidden") {
      this.transition("hidden");
      if (this.idleTimer !== null) {
        this.env.clearTimeout(this.idleTimer);
        this.idleTimer = null;
      }
    } else {
      // Returning to visible — reset idle timer, go active
      this.lastActivity = Date.now();
      this.resetIdleTimer();
      this.transition("active");
    }
  }

  private resetIdleTimer(): void {
    if (this.idleTimer !== null) {
      this.env.clearTimeout(this.idleTimer);
    }
    this.idleTimer = this.env.setTimeout(() => {
      this.idleTimer = null;
      if (this.state === "active") {
        this.transition("idle");
      }
    }, this.idleTimeoutMs);
  }

  private transition(newState: LivenessState): void {
    if (this.state === newState) return;
    this.state = newState;
    this.onChange(newState);
  }
}

/**
 * Creates a LivenessEnvironment backed by the real browser DOM.
 * Call this only in browser contexts.
 */
export function browserEnvironment(): LivenessEnvironment {
  return {
    addEventListener(target, event, handler) {
      const el = target === "document" ? document : window;
      el.addEventListener(event, handler, { passive: true });
    },
    removeEventListener(target, event, handler) {
      const el = target === "document" ? document : window;
      el.removeEventListener(event, handler);
    },
    getVisibilityState() {
      return document.visibilityState as "visible" | "hidden";
    },
    setTimeout: (fn, ms) => window.setTimeout(fn, ms),
    clearTimeout: (id) => window.clearTimeout(id),
  };
}
