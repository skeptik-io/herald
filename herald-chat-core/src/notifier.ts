type Listener = () => void;

/**
 * Change notification bus that implements the useSyncExternalStore contract.
 *
 * Slice keys follow the pattern:
 *   "messages:{streamId}", "typing:{streamId}", "unread:{streamId}",
 *   "unread:total", "members:{streamId}", "scroll:{streamId}", "liveness"
 */
export class Notifier {
  private listeners = new Map<string, Set<Listener>>();

  subscribe(slice: string, listener: Listener): () => void {
    let set = this.listeners.get(slice);
    if (!set) {
      set = new Set();
      this.listeners.set(slice, set);
    }
    set.add(listener);
    return () => {
      set!.delete(listener);
      if (set!.size === 0) {
        this.listeners.delete(slice);
      }
    };
  }

  notify(slice: string): void {
    const set = this.listeners.get(slice);
    if (set) {
      for (const listener of set) {
        try {
          listener();
        } catch (err) {
          console.error(`[herald-chat] Notifier listener error on "${slice}":`, err);
        }
      }
    }
  }

  notifyMany(slices: string[]): void {
    for (const slice of slices) {
      this.notify(slice);
    }
  }

  clear(): void {
    this.listeners.clear();
  }
}
