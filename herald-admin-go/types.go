package herald

// Stream represents a Herald stream.
type Stream struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Meta      any    `json:"meta,omitempty"`
	Public    bool   `json:"public"`
	Archived  bool   `json:"archived"`
	CreatedAt int64  `json:"created_at"`
}

// Member represents a user's membership in a stream.
type Member struct {
	StreamID string `json:"stream_id"`
	UserID   string `json:"user_id"`
	Role     string `json:"role"`
	JoinedAt int64  `json:"joined_at"`
}

// Event represents a stream event.
type Event struct {
	ID       string `json:"id"`
	Stream   string `json:"stream"`
	Seq      uint64 `json:"seq"`
	Sender   string `json:"sender"`
	Body     string `json:"body"`
	Meta     any    `json:"meta,omitempty"`
	ParentID string `json:"parent_id,omitempty"`
	EditedAt *int64 `json:"edited_at,omitempty"`
	SentAt   int64  `json:"sent_at"`
}

// ReactionSummary represents aggregated reactions on an event.
type ReactionSummary struct {
	Emoji string   `json:"emoji"`
	Count int      `json:"count"`
	Users []string `json:"users"`
}

// EventList is a paginated list of events.
type EventList struct {
	Events  []Event `json:"events"`
	HasMore bool    `json:"has_more"`
}

// EventPublishResult is the response from publishing an event.
type EventPublishResult struct {
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

// MemberPresenceEntry is a member's presence in a stream.
type MemberPresenceEntry struct {
	UserID string `json:"user_id"`
	Status string `json:"status"`
}

// Cursor is a user's read position in a stream.
type Cursor struct {
	UserID string `json:"user_id"`
	Seq    uint64 `json:"seq"`
}

// HealthResponse is the response from the health endpoint.
type HealthResponse struct {
	Status      string `json:"status"`
	Connections int    `json:"connections"`
	Streams     int    `json:"streams"`
	UptimeSecs  int64  `json:"uptime_secs"`
}

// Tenant represents a Herald tenant.
type Tenant struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Plan      string `json:"plan"`
	CreatedAt int64  `json:"created_at"`
}

// AuditEvent represents a single audit log entry.
type AuditEvent struct {
	ID           string `json:"id"`
	Timestamp    int64  `json:"timestamp"`
	Operation    string `json:"operation"`
	ResourceType string `json:"resource_type"`
	ResourceID   string `json:"resource_id"`
	Actor        string `json:"actor"`
	Result       string `json:"result"`
	TenantID     string `json:"tenant_id"`
	Diff         any    `json:"diff,omitempty"`
	Metadata     any    `json:"metadata,omitempty"`
}

// AuditQueryResult is the response from querying audit events.
type AuditQueryResult struct {
	Events  []AuditEvent `json:"events"`
	Matched int          `json:"matched"`
}

// AuditCountResult is the response from counting audit events.
type AuditCountResult struct {
	Count int `json:"count"`
}

// AuditQueryOptions are optional filter parameters for audit queries.
type AuditQueryOptions struct {
	Operation    *string
	ResourceType *string
	ResourceID   *string
	Actor        *string
	Result       *string
	Since        *string
	Until        *string
	Limit        *int
}
