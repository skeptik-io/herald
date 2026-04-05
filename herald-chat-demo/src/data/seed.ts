export interface User {
  id: string;
  name: string;
  avatar: string;
  presence: "online" | "away" | "dnd" | "offline";
}

export interface Message {
  id: string;
  stream: string;
  seq: number;
  sender: string;
  body: string;
  sent_at: number;
  parent_id?: string;
}

export interface Stream {
  id: string;
  name: string;
  kind: "dm" | "group";
  members: string[];
}

// ── Users ──────────────────────────────────────────────────────────────

export const CURRENT_USER_ID = "alice";

export const users: Record<string, User> = {
  alice: { id: "alice", name: "Alice Chen", avatar: "AC", presence: "online" },
  bob: { id: "bob", name: "Bob Park", avatar: "BP", presence: "online" },
  carol: { id: "carol", name: "Carol Wu", avatar: "CW", presence: "away" },
  dave: { id: "dave", name: "Dave Kim", avatar: "DK", presence: "dnd" },
  eve: { id: "eve", name: "Eve Santos", avatar: "ES", presence: "offline" },
};

// ── Streams (conversations) ────────────────────────────────────────────

export const streams: Stream[] = [
  { id: "dm-alice-bob", name: "Bob Park", kind: "dm", members: ["alice", "bob"] },
  { id: "dm-alice-carol", name: "Carol Wu", kind: "dm", members: ["alice", "carol"] },
  { id: "group-eng", name: "Engineering Team", kind: "group", members: ["alice", "bob", "carol", "dave"] },
  { id: "group-design", name: "Design Review", kind: "group", members: ["alice", "carol", "eve"] },
  { id: "dm-alice-dave", name: "Dave Kim", kind: "dm", members: ["alice", "dave"] },
];

// ── Helper ─────────────────────────────────────────────────────────────

const BASE_TIME = Date.now() - 3_600_000; // 1 hour ago
let msgSeq = 0;

function msg(stream: string, sender: string, body: string, minutesAgo: number): Message {
  msgSeq++;
  return {
    id: `msg-${msgSeq}`,
    stream,
    seq: msgSeq,
    sender,
    body,
    sent_at: BASE_TIME + (60 - minutesAgo) * 60_000,
  };
}

// ── Pre-seeded messages ────────────────────────────────────────────────

export const seedMessages: Message[] = [
  // DM with Bob
  msg("dm-alice-bob", "bob", "Hey Alice, did you see the new Herald SDK release?", 55),
  msg("dm-alice-bob", "alice", "Yes! The typing indicators are really smooth", 53),
  msg("dm-alice-bob", "bob", "I integrated it into our app yesterday. Presence works great too.", 50),
  msg("dm-alice-bob", "alice", "Nice. Did you run into any issues with reconnection?", 48),
  msg("dm-alice-bob", "bob", "Nope, the automatic re-subscribe on reconnect handled it perfectly.", 45),
  msg("dm-alice-bob", "bob", "Want to pair on the read receipts feature tomorrow?", 10),

  // DM with Carol
  msg("dm-alice-carol", "carol", "The design mockups are ready for review", 40),
  msg("dm-alice-carol", "alice", "Great, I'll take a look this afternoon", 38),
  msg("dm-alice-carol", "carol", "Take your time! No rush.", 37),
  msg("dm-alice-carol", "carol", "Also, can you check the color tokens in the new theme?", 5),

  // Engineering Team
  msg("group-eng", "dave", "CI pipeline is green after the fix", 30),
  msg("group-eng", "bob", "Nice work Dave! That was a tricky one.", 28),
  msg("group-eng", "alice", "Agreed. Should we add a regression test?", 26),
  msg("group-eng", "dave", "Already on it. PR coming soon.", 24),
  msg("group-eng", "carol", "Sprint retro at 3pm today, don't forget!", 8),
  msg("group-eng", "bob", "Thanks for the reminder Carol", 7),

  // Design Review
  msg("group-design", "eve", "I uploaded the new component library to Figma", 20),
  msg("group-design", "carol", "Love the new spacing system", 18),
  msg("group-design", "alice", "The typography scale looks perfect", 16),
  msg("group-design", "eve", "Thanks! I'll finalize the icon set next week", 15),

  // DM with Dave
  msg("dm-alice-dave", "dave", "Quick question about the auth module", 35),
  msg("dm-alice-dave", "alice", "Sure, what's up?", 34),
  msg("dm-alice-dave", "dave", "Is the JWT refresh flow using Herald's token_expiring event?", 33),
  msg("dm-alice-dave", "alice", "Yes, the onTokenExpiring callback handles it automatically", 32),
  msg("dm-alice-dave", "dave", "Perfect, thanks!", 31),
];

// ── Cursor positions (read receipts) ─────────────────────────────────
// Maps stream -> user -> last read seq number

export const seedCursors: Record<string, Record<string, number>> = {
  "dm-alice-bob": {
    alice: 5,  // Alice hasn't read Bob's latest message (seq 6)
    bob: 6,
  },
  "dm-alice-carol": {
    alice: 9,  // Alice hasn't read Carol's latest (seq 10)
    carol: 10,
  },
  "group-eng": {
    alice: 14,  // Alice hasn't read the last 2 messages
    bob: 16,
    carol: 16,
    dave: 16,
  },
  "group-design": {
    alice: 20,
    carol: 20,
    eve: 20,
  },
  "dm-alice-dave": {
    alice: 25,
    dave: 25,
  },
};
