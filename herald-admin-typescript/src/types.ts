export interface Stream {
  id: string;
  name: string;
  meta?: unknown;
  created_at: number;
  public?: boolean;
  archived?: boolean;
}

export interface Member {
  stream_id: string;
  user_id: string;
  role: string;
  joined_at: number;
}

export interface Event {
  id: string;
  stream: string;
  seq: number;
  sender: string;
  body: string;
  meta?: unknown;
  sent_at: number;
  parent_id?: string;
  edited_at?: number;
}

export interface ReactionSummary {
  emoji: string;
  count: number;
  users: string[];
}

export interface BlockList {
  blocked: string[];
}

export interface EventList {
  events: Event[];
  has_more: boolean;
}

export interface UserPresence {
  user_id: string;
  status: string;
  connections: number;
}

export interface MemberPresenceEntry {
  user_id: string;
  status: string;
}

export interface Cursor {
  user_id: string;
  seq: number;
}

export interface HealthResponse {
  status: string;
  connections: number;
  streams: number;
  uptime_secs: number;
}

export interface EventPublishResult {
  id: string;
  seq: number;
  sent_at: number;
}
