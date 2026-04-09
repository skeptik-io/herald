# @skeptik-io/herald-chat-react

React bindings for Herald Chat. Hooks use `useSyncExternalStore` for tear-free reads. Components are headless (render-prop pattern, zero styling).

## Install

```bash
npm install @skeptik-io/herald-chat-react --registry=https://npm.pkg.github.com
```

Peer dependencies: `@skeptik-io/herald-chat ^0.1.0`, `@skeptik-io/herald-chat-sdk ^0.1.0`, `react ^18.0.0 || ^19.0.0`

## Provider

Wrap your app with `HeraldChatProvider`. It creates a `ChatCore` instance internally and manages its lifecycle.

```tsx
import { HeraldChatProvider } from '@skeptik-io/herald-chat-react';

function App() {
  return (
    <HeraldChatProvider
      client={heraldClient}
      chat={heraldChatClient}
      userId="alice"
      liveness={{ idleTimeoutMs: 120_000 }}
      scrollIdleMs={1000}
      middleware={[
        (event, next) => {
          // code before next() runs pre-store-mutation
          next();
          // code after next() runs post-store-mutation
        },
      ]}
    >
      <Chat />
    </HeraldChatProvider>
  );
}
```

## Hooks

All hooks are null-safe — they return stable defaults (`[]`, `0`, `undefined`) when called outside a `HeraldChatProvider`. This lets you render components before the provider mounts (e.g., while the WebSocket connects) without conditional logic. Write actions (`send`, `edit`, `deleteEvent`) reject with an error if called before the provider is available.

### useMessages

```typescript
const { messages, send, edit, deleteEvent, loadMore } = useMessages('general');

// messages: Message[] — sorted by seq, optimistic messages at end
// send(body, opts?) → Promise<PendingMessage>
// edit(eventId, body) → Promise<void>
// deleteEvent(eventId) → Promise<void>
// loadMore() → Promise<boolean> — returns false when no more history
```

### useMembers

```typescript
const members = useMembers('general');
// Member[] — { userId, role, presence }
```

### useTyping

```typescript
const { typing, sendTyping } = useTyping('general');
// typing: string[] — user IDs currently typing
// sendTyping() — sends typing.start frame
```

### useUnreadCount / useTotalUnreadCount

```typescript
const unread = useUnreadCount('general');       // number
const totalUnread = useTotalUnreadCount();      // number (all streams)
```

### useLiveness / usePresence

```typescript
const liveness = useLiveness();   // "active" | "idle" | "hidden"
const presence = usePresence();   // "online" | "away" (derived from liveness)
```

### useEphemeral

```typescript
const last = useEphemeral('general');                  // most recent ephemeral event
const ping = useEphemeral('general', 'custom.ping');   // filtered by event type
// EventReceived | undefined
```

## Components

All components use the render-prop pattern. They compose hooks internally and pass state to `children`.

### HeraldChat

Scroll coordination: live-edge detection, load-more triggers, scroll anchoring on history prepend.

```tsx
<HeraldChat
  streamId="general"
  scrollRef={scrollContainerRef}
  edgeThreshold={50}
  loadMoreThreshold={100}
>
  {({ pendingCount, atLiveEdge, isLoadingMore, scrollToBottom }) => (
    <>
      {!atLiveEdge && (
        <button onClick={scrollToBottom}>
          {pendingCount} new messages
        </button>
      )}
    </>
  )}
</HeraldChat>
```

### MessageList

```tsx
<MessageList streamId="general">
  {({ messages, send, edit, deleteEvent, loadMore }) => (
    <div>
      {messages.map(msg => <div key={msg.id}>{msg.body}</div>)}
    </div>
  )}
</MessageList>
```

### MessageInput

```tsx
<MessageInput streamId="general">
  {({ send, sendTyping }) => (
    <input
      onChange={() => sendTyping()}
      onKeyDown={(e) => {
        if (e.key === 'Enter') send(e.currentTarget.value);
      }}
    />
  )}
</MessageInput>
```

### PresenceIndicator

```tsx
<PresenceIndicator streamId="general">
  {({ members, onlineCount }) => (
    <span>{onlineCount} online</span>
  )}
</PresenceIndicator>
```

### TypingIndicator

```tsx
<TypingIndicator streamId="general">
  {({ typing, isAnyoneTyping }) =>
    isAnyoneTyping ? <span>{typing.join(', ')} typing...</span> : null
  }
</TypingIndicator>
```

## Direct ChatCore Access

For advanced use cases, access the underlying `ChatCore` instance. Returns `null` when called outside a `HeraldChatProvider`.

```typescript
import { useChatCore } from '@skeptik-io/herald-chat-react';

function CustomComponent() {
  const core = useChatCore();
  if (!core) return null; // provider not mounted yet
  // Use core.subscribe(), core.getMessages(), etc. directly
}
```
