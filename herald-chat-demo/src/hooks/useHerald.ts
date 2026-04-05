/**
 * React hooks wrapping Herald client patterns.
 *
 * In production you'd import { HeraldClient } from "@anthropic/herald-sdk"
 * and pass it via context. Here we use MockHeraldClient with the same API.
 */

import { createContext, useContext, useCallback, useEffect, useRef, useState } from "react";
import type { MockHeraldClient, EventNew, PresenceChanged, CursorMoved, TypingEvent } from "../mock/herald-mock";
import { CURRENT_USER_ID, streams, users, type User } from "../data/seed";

// ── Context ────────────────────────────────────────────────────────────

export const HeraldContext = createContext<MockHeraldClient | null>(null);

export function useHeraldClient(): MockHeraldClient {
  const client = useContext(HeraldContext);
  if (!client) throw new Error("HeraldContext not provided");
  return client;
}

// ── useMessages: live message list for a stream ───────────────────────

export function useMessages(streamId: string) {
  const client = useHeraldClient();
  const [messages, setMessages] = useState<EventNew[]>([]);

  useEffect(() => {
    let mounted = true;

    // Fetch history
    client.fetch(streamId).then((batch) => {
      if (mounted) setMessages(batch.events);
    });

    // Listen for new events
    const handler = (evt: unknown) => {
      const e = evt as EventNew;
      if (e.stream === streamId) {
        setMessages((prev) => [...prev, e]);
      }
    };
    client.on("event", handler);
    return () => {
      mounted = false;
      client.off("event", handler);
    };
  }, [client, streamId]);

  return messages;
}

// ── usePresence: track user presence globally ─────────────────────────

export function usePresence() {
  const client = useHeraldClient();
  const [presence, setPresence] = useState<Record<string, string>>(() => {
    const initial: Record<string, string> = {};
    for (const u of Object.values(users)) {
      initial[u.id] = u.presence;
    }
    return initial;
  });

  useEffect(() => {
    const handler = (evt: unknown) => {
      const e = evt as PresenceChanged;
      setPresence((prev) => ({ ...prev, [e.user_id]: e.presence }));
    };
    client.on("presence", handler);
    return () => client.off("presence", handler);
  }, [client]);

  return presence;
}

// ── useTyping: typing indicators for a stream ─────────────────────────

export function useTyping(streamId: string) {
  const client = useHeraldClient();
  const [typingUsers, setTypingUsers] = useState<Set<string>>(new Set());
  const timers = useRef(new Map<string, ReturnType<typeof setTimeout>>());

  useEffect(() => {
    const handler = (evt: unknown) => {
      const e = evt as TypingEvent;
      if (e.stream !== streamId || e.user_id === CURRENT_USER_ID) return;

      if (e.active) {
        setTypingUsers((prev) => new Set(prev).add(e.user_id));
        // Auto-clear after 5s (Herald server auto-expires typing after 5s)
        const existing = timers.current.get(e.user_id);
        if (existing) clearTimeout(existing);
        timers.current.set(
          e.user_id,
          setTimeout(() => {
            setTypingUsers((prev) => {
              const next = new Set(prev);
              next.delete(e.user_id);
              return next;
            });
            timers.current.delete(e.user_id);
          }, 5000),
        );
      } else {
        setTypingUsers((prev) => {
          const next = new Set(prev);
          next.delete(e.user_id);
          return next;
        });
        const existing = timers.current.get(e.user_id);
        if (existing) clearTimeout(existing);
        timers.current.delete(e.user_id);
      }
    };

    client.on("typing", handler);
    return () => {
      client.off("typing", handler);
      for (const t of timers.current.values()) clearTimeout(t);
      timers.current.clear();
    };
  }, [client, streamId]);

  return typingUsers;
}

// ── useCursors: read receipts for a stream ────────────────────────────

export function useCursors(streamId: string) {
  const client = useHeraldClient();
  const [cursors, setCursors] = useState<Record<string, number>>(() =>
    client.getCursors(streamId),
  );

  useEffect(() => {
    // Reset cursors when stream changes
    setCursors(client.getCursors(streamId));

    const handler = (evt: unknown) => {
      const e = evt as CursorMoved;
      if (e.stream === streamId) {
        setCursors((prev) => ({ ...prev, [e.user_id]: e.seq }));
      }
    };
    client.on("cursor", handler);
    return () => client.off("cursor", handler);
  }, [client, streamId]);

  return cursors;
}

// ── useUnreadCounts: unread count per stream ──────────────────────────

export function useUnreadCounts() {
  const client = useHeraldClient();

  const computeCounts = useCallback(() => {
    const counts: Record<string, number> = {};
    for (const s of streams) {
      counts[s.id] = client.getUnreadCount(s.id);
    }
    return counts;
  }, [client]);

  const [counts, setCounts] = useState(computeCounts);

  useEffect(() => {
    const onEvent = () => setCounts(computeCounts());
    const onCursor = () => setCounts(computeCounts());

    client.on("event", onEvent);
    client.on("cursor", onCursor);
    return () => {
      client.off("event", onEvent);
      client.off("cursor", onCursor);
    };
  }, [client, computeCounts]);

  return counts;
}

// ── useSendTyping: debounced typing indicator ─────────────────────────

export function useSendTyping(streamId: string) {
  const client = useHeraldClient();
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isTyping = useRef(false);

  const onKeyStroke = useCallback(() => {
    if (!isTyping.current) {
      isTyping.current = true;
      client.startTyping(streamId);
    }
    // Reset the stop timer
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      isTyping.current = false;
      client.stopTyping(streamId);
    }, 2000);
  }, [client, streamId]);

  // Cleanup on unmount or stream change
  useEffect(() => {
    return () => {
      if (timer.current) clearTimeout(timer.current);
      if (isTyping.current) {
        client.stopTyping(streamId);
        isTyping.current = false;
      }
    };
  }, [client, streamId]);

  return onKeyStroke;
}
