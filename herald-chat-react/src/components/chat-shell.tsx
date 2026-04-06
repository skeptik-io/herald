import { useRef, useCallback, useLayoutEffect, type ReactNode, type RefObject } from "react";
import { useSyncExternalStore } from "react";
import type { ScrollStateSnapshot } from "herald-chat";
import { useChatCore } from "../context.js";

export interface ChatShellProps {
  streamId: string;
  children: (state: ChatShellState) => ReactNode;
  /** Ref to the scrollable container element. */
  scrollRef: RefObject<HTMLElement | null>;
  /** Distance from bottom (px) to consider "at live edge". Default 50. */
  edgeThreshold?: number;
  /** Threshold from top (px) to trigger load-more. Default 100. */
  loadMoreThreshold?: number;
}

export interface ChatShellState {
  pendingCount: number;
  atLiveEdge: boolean;
  isLoadingMore: boolean;
  scrollToBottom(): void;
}

export function ChatShell({
  streamId,
  children,
  scrollRef,
  edgeThreshold = 50,
  loadMoreThreshold = 100,
}: ChatShellProps) {
  const core = useChatCore();
  const prevScrollHeight = useRef(0);
  const isAnchoring = useRef(false);

  const scrollState = useSyncExternalStore(
    (cb) => core.subscribe(`scroll:${streamId}`, cb),
    () => core.getScrollState(streamId),
    () => DEFAULT_SCROLL,
  );

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el || isAnchoring.current) return;

    const distFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    const atEdge = distFromBottom <= edgeThreshold;
    core.setAtLiveEdge(streamId, atEdge);

    if (el.scrollTop <= loadMoreThreshold) {
      core.loadMore(streamId);
    }
  }, [core, streamId, scrollRef, edgeThreshold, loadMoreThreshold]);

  // Scroll anchoring: preserve position when history is prepended
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;

    const newScrollHeight = el.scrollHeight;
    if (newScrollHeight > prevScrollHeight.current && prevScrollHeight.current > 0) {
      const delta = newScrollHeight - prevScrollHeight.current;
      if (el.scrollTop < loadMoreThreshold + 50) {
        isAnchoring.current = true;
        el.scrollTop += delta;
        requestAnimationFrame(() => {
          isAnchoring.current = false;
        });
      }
    }
    prevScrollHeight.current = newScrollHeight;
  });

  // Attach scroll listener
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.addEventListener("scroll", handleScroll, { passive: true });
    return () => el.removeEventListener("scroll", handleScroll);
  }, [handleScroll]);

  const scrollToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (el) {
      el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
    }
  }, [scrollRef]);

  return (
    <>
      {children({
        pendingCount: scrollState.pendingCount,
        atLiveEdge: scrollState.atLiveEdge,
        isLoadingMore: scrollState.isLoadingMore,
        scrollToBottom,
      })}
    </>
  );
}

const DEFAULT_SCROLL: ScrollStateSnapshot = {
  atLiveEdge: true,
  pendingCount: 0,
  isLoadingMore: false,
};
