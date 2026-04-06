import { useMemo, type ReactNode } from "react";
import { useTyping } from "../hooks/use-typing.js";

export interface TypingIndicatorProps {
  streamId: string;
  children: (state: TypingIndicatorState) => ReactNode;
}

export interface TypingIndicatorState {
  typing: string[];
  isAnyoneTyping: boolean;
}

export function TypingIndicator({ streamId, children }: TypingIndicatorProps) {
  const { typing } = useTyping(streamId);

  const isAnyoneTyping = useMemo(() => typing.length > 0, [typing]);

  return <>{children({ typing, isAnyoneTyping })}</>;
}
