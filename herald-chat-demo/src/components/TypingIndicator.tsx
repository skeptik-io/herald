import { users } from "../data/seed";

interface TypingIndicatorProps {
  typingUsers: Set<string>;
}

export function TypingIndicator({ typingUsers }: TypingIndicatorProps) {
  if (typingUsers.size === 0) return null;

  const names = Array.from(typingUsers).map(
    (uid) => users[uid]?.name.split(" ")[0] ?? uid,
  );

  let text: string;
  if (names.length === 1) {
    text = `${names[0]} is typing`;
  } else if (names.length === 2) {
    text = `${names[0]} and ${names[1]} are typing`;
  } else {
    text = `${names.length} people are typing`;
  }

  return (
    <div className="typing-indicator">
      <div className="typing-dots">
        <span className="dot" />
        <span className="dot" />
        <span className="dot" />
      </div>
      <span className="typing-text">{text}</span>
    </div>
  );
}
