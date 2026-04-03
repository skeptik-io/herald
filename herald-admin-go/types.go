package herald

// Room represents a Herald chat room.
type Room struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Meta      any    `json:"meta,omitempty"`
	Public    bool   `json:"public"`
	Archived  bool   `json:"archived"`
	CreatedAt int64  `json:"created_at"`
}

// Member represents a user's membership in a room.
type Member struct {
	RoomID   string `json:"room_id"`
	UserID   string `json:"user_id"`
	Role     string `json:"role"`
	JoinedAt int64  `json:"joined_at"`
}

// Message represents a chat message.
type Message struct {
	ID       string `json:"id"`
	Room     string `json:"room"`
	Seq      uint64 `json:"seq"`
	Sender   string `json:"sender"`
	Body     string `json:"body"`
	Meta     any    `json:"meta,omitempty"`
	ParentID string `json:"parent_id,omitempty"`
	EditedAt *int64 `json:"edited_at,omitempty"`
	SentAt   int64  `json:"sent_at"`
}

// ReactionSummary represents aggregated reactions on a message.
type ReactionSummary struct {
	Emoji string   `json:"emoji"`
	Count int      `json:"count"`
	Users []string `json:"users"`
}

// MessageList is a paginated list of messages.
type MessageList struct {
	Messages []Message `json:"messages"`
	HasMore  bool      `json:"has_more"`
}

// MessageSendResult is the response from injecting a message.
type MessageSendResult struct {
	ID     string `json:"id"`
	Seq    uint64 `json:"seq"`
	SentAt int64  `json:"sent_at"`
}

// UserPresence is a user's presence status.
type UserPresence struct {
	UserID      string `json:"user_id"`
	Status      string `json:"status"`
	Connections int    `json:"connections"`
}

// MemberPresenceEntry is a member's presence in a room.
type MemberPresenceEntry struct {
	UserID string `json:"user_id"`
	Status string `json:"status"`
}

// Cursor is a user's read position in a room.
type Cursor struct {
	UserID string `json:"user_id"`
	Seq    uint64 `json:"seq"`
}

// HealthResponse is the response from the health endpoint.
type HealthResponse struct {
	Status      string `json:"status"`
	Connections int    `json:"connections"`
	Rooms       int    `json:"rooms"`
	UptimeSecs  int64  `json:"uptime_secs"`
}

// Tenant represents a Herald tenant.
type Tenant struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Plan      string `json:"plan"`
	CreatedAt int64  `json:"created_at"`
}
