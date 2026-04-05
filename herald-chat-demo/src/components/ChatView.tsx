import { useState, useRef, useEffect } from "react";
import { streams, users, CURRENT_USER_ID } from "../data/seed";
import {
  useHeraldClient,
  useMessages,
  useTyping,
  useCursors,
  usePresence,
  useSendTyping,
} from "../hooks/useHerald";
import { TypingIndicator } from "./TypingIndicator";
import { ReadReceipts } from "./ReadReceipts";
import { PresenceIndicator } from "./PresenceIndicator";

interface ChatViewProps {
  streamId: string;
}

function formatTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function ChatView({ streamId }: ChatViewProps) {
  const client = useHeraldClient();
  const messages = useMessages(streamId);
  const typingUsers = useTyping(streamId);
  const cursors = useCursors(streamId);
  const presence = usePresence();
  const onKeyStroke = useSendTyping(streamId);
  const [input, setInput] = useState("");
  const bottomRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const stream = streams.find((s) => s.id === streamId)!;

  // Auto-scroll to bottom on new messages
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, typingUsers]);

  // Mark messages as read when viewing a stream
  useEffect(() => {
    if (messages.length > 0) {
      const latestSeq = messages[messages.length - 1].seq;
      client.updateCursor(streamId, latestSeq);
    }
  }, [client, streamId, messages]);

  // Focus input on stream change
  useEffect(() => {
    inputRef.current?.focus();
  }, [streamId]);

  const handleSend = () => {
    const body = input.trim();
    if (!body) return;
    client.publish(streamId, body);
    setInput("");
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  // Header info
  const otherMembers = stream.members.filter((m) => m !== CURRENT_USER_ID);
  const headerPresence = stream.kind === "dm" ? presence[otherMembers[0]] : undefined;

  return (
    <div className="chat-view">
      {/* Header */}
      <header className="chat-header">
        <div className="chat-header-info">
          <h2 className="chat-header-name">
            {stream.kind === "group" ? `# ${stream.name}` : stream.name}
          </h2>
          {stream.kind === "dm" && headerPresence && (
            <div className="chat-header-status">
              <PresenceIndicator status={headerPresence} size="sm" />
              <span className="status-text">{headerPresence}</span>
            </div>
          )}
          {stream.kind === "group" && (
            <div className="chat-header-status">
              <span className="status-text">
                {otherMembers.length + 1} members
                {" \u00b7 "}
                {otherMembers.filter((m) => presence[m] === "online").length + 1} online
              </span>
            </div>
          )}
        </div>
        {stream.kind === "group" && (
          <div className="chat-header-members">
            {stream.members.map((uid) => (
              <span key={uid} className="member-pip" title={`${users[uid]?.name} (${presence[uid]})`}>
                <span className="pip-avatar">{users[uid]?.avatar}</span>
                <PresenceIndicator status={presence[uid] ?? "offline"} size="sm" />
              </span>
            ))}
          </div>
        )}
      </header>

      {/* Messages */}
      <div className="chat-messages">
        {messages.map((msg, i) => {
          const isMine = msg.sender === CURRENT_USER_ID;
          const user = users[msg.sender];
          const showAvatar =
            i === 0 || messages[i - 1].sender !== msg.sender;

          return (
            <div
              key={msg.id}
              className={`message ${isMine ? "message-mine" : "message-theirs"} ${showAvatar ? "message-grouped-first" : "message-grouped"}`}
            >
              {!isMine && showAvatar && (
                <div className="message-avatar">
                  <span className="avatar-text">{user?.avatar ?? "?"}</span>
                </div>
              )}
              {!isMine && !showAvatar && <div className="message-avatar-spacer" />}
              <div className="message-content">
                {showAvatar && (
                  <div className="message-meta">
                    <span className="message-sender">{user?.name ?? msg.sender}</span>
                    <span className="message-time">{formatTime(msg.sent_at)}</span>
                  </div>
                )}
                <div className={`message-bubble ${isMine ? "bubble-mine" : "bubble-theirs"}`}>
                  {msg.body}
                </div>
                {isMine && (
                  <ReadReceipts
                    seq={msg.seq}
                    cursors={cursors}
                    memberIds={stream.members}
                  />
                )}
              </div>
            </div>
          );
        })}

        <TypingIndicator typingUsers={typingUsers} />
        <div ref={bottomRef} />
      </div>

      {/* Input */}
      <div className="chat-input-container">
        <input
          ref={inputRef}
          className="chat-input"
          type="text"
          placeholder={`Message ${stream.kind === "group" ? "#" + stream.name : stream.name}...`}
          value={input}
          onChange={(e) => {
            setInput(e.target.value);
            onKeyStroke();
          }}
          onKeyDown={handleKeyDown}
        />
        <button
          className="send-button"
          onClick={handleSend}
          disabled={!input.trim()}
        >
          Send
        </button>
      </div>
    </div>
  );
}
