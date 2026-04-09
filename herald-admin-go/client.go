package herald

import (
	"context"
	"encoding/json"
	"fmt"
	"net/url"
)

// Options configures the Herald admin client.
type Options struct {
	// URL is the Herald HTTP API URL (e.g., http://localhost:6201).
	URL string
	// Token is the API bearer token.
	Token string
	// TenantID is the tenant ID used for tenant-scoped admin operations (e.g., audit).
	TenantID string
}

// ChatNamespaces groups chat-specific (conversational layer) namespaces.
type ChatNamespaces struct {
	Presence *PresenceNamespace
	Blocks   *BlockNamespace
}

// HeraldAdmin is the Herald HTTP admin client.
type HeraldAdmin struct {
	// Core namespaces (event transport)
	Streams *StreamNamespace
	Members *MemberNamespace
	Events  *EventNamespace
	Tenants *TenantNamespace
	Audit   *AuditNamespace

	// Deprecated: Use Chat.Presence instead.
	Presence *PresenceNamespace
	// Deprecated: Use Chat.Blocks instead.
	Blocks *BlockNamespace

	// Chat groups chat-specific namespaces (conversational layer).
	Chat *ChatNamespaces

	transport *httpTransport
}

// New creates a new Herald admin client.
func New(opts Options) *HeraldAdmin {
	t := newTransport(opts.URL, opts.Token)
	presence := &PresenceNamespace{t: t}
	blocks := &BlockNamespace{t: t}
	return &HeraldAdmin{
		Streams:   &StreamNamespace{t: t},
		Members:   &MemberNamespace{t: t},
		Events:    &EventNamespace{t: t},
		Presence:  presence,
		Tenants:   &TenantNamespace{t: t},
		Audit:     &AuditNamespace{t: t, tenantID: opts.TenantID},
		Blocks:    blocks,
		Chat:      &ChatNamespaces{Presence: presence, Blocks: blocks},
		transport: t,
	}
}

// Health checks the Herald server status.
func (h *HeraldAdmin) Health(ctx context.Context) (*HealthResponse, error) {
	data, err := h.transport.request(ctx, "GET", "/health", nil)
	if err != nil {
		return nil, err
	}
	var resp HealthResponse
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, fmt.Errorf("unmarshal health: %w", err)
	}
	return &resp, nil
}

// StreamNamespace provides stream management operations.
type StreamNamespace struct{ t *httpTransport }

// StreamCreateOptions are optional parameters for stream creation.
type StreamCreateOptions struct {
	Meta   any  `json:"meta,omitempty"`
	Public bool `json:"public,omitempty"`
}

func (ns *StreamNamespace) Create(ctx context.Context, id, name string, opts *StreamCreateOptions) (*Stream, error) {
	body := map[string]any{"id": id, "name": name}
	if opts != nil {
		if opts.Meta != nil {
			body["meta"] = opts.Meta
		}
		if opts.Public {
			body["public"] = true
		}
	}
	data, err := ns.t.request(ctx, "POST", "/streams", body)
	if err != nil {
		return nil, err
	}
	var stream Stream
	if err := json.Unmarshal(data, &stream); err != nil {
		return nil, err
	}
	return &stream, nil
}

func (ns *StreamNamespace) Get(ctx context.Context, id string) (*Stream, error) {
	data, err := ns.t.request(ctx, "GET", "/streams/"+url.PathEscape(id), nil)
	if err != nil {
		return nil, err
	}
	var stream Stream
	if err := json.Unmarshal(data, &stream); err != nil {
		return nil, err
	}
	return &stream, nil
}

func (ns *StreamNamespace) Update(ctx context.Context, id string, name *string, meta any, archived *bool) error {
	body := map[string]any{}
	if name != nil {
		body["name"] = *name
	}
	if meta != nil {
		body["meta"] = meta
	}
	if archived != nil {
		body["archived"] = *archived
	}
	_, err := ns.t.request(ctx, "PATCH", "/streams/"+url.PathEscape(id), body)
	return err
}

func (ns *StreamNamespace) List(ctx context.Context) ([]Stream, error) {
	data, err := ns.t.request(ctx, "GET", "/streams", nil)
	if err != nil {
		return nil, err
	}
	var resp struct {
		Streams []Stream `json:"streams"`
	}
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, err
	}
	return resp.Streams, nil
}

func (ns *StreamNamespace) Delete(ctx context.Context, id string) error {
	_, err := ns.t.request(ctx, "DELETE", "/streams/"+url.PathEscape(id), nil)
	return err
}

// MemberNamespace provides member management operations.
type MemberNamespace struct{ t *httpTransport }

func (ns *MemberNamespace) Add(ctx context.Context, streamID, userID string, role string) (*Member, error) {
	body := map[string]string{"user_id": userID}
	if role != "" {
		body["role"] = role
	}
	data, err := ns.t.request(ctx, "POST", "/streams/"+url.PathEscape(streamID)+"/members", body)
	if err != nil {
		return nil, err
	}
	var m Member
	if err := json.Unmarshal(data, &m); err != nil {
		return nil, err
	}
	return &m, nil
}

func (ns *MemberNamespace) List(ctx context.Context, streamID string) ([]Member, error) {
	data, err := ns.t.request(ctx, "GET", "/streams/"+url.PathEscape(streamID)+"/members", nil)
	if err != nil {
		return nil, err
	}
	var resp struct {
		Members []Member `json:"members"`
	}
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, err
	}
	return resp.Members, nil
}

func (ns *MemberNamespace) Remove(ctx context.Context, streamID, userID string) error {
	_, err := ns.t.request(ctx, "DELETE", "/streams/"+url.PathEscape(streamID)+"/members/"+url.PathEscape(userID), nil)
	return err
}

func (ns *MemberNamespace) Update(ctx context.Context, streamID, userID, role string) error {
	_, err := ns.t.request(ctx, "PATCH", "/streams/"+url.PathEscape(streamID)+"/members/"+url.PathEscape(userID), map[string]string{"role": role})
	return err
}

// EventNamespace provides event operations.
type EventNamespace struct{ t *httpTransport }

// EventPublishOptions are optional parameters for publishing an event.
type EventPublishOptions struct {
	Meta              any    `json:"meta,omitempty"`
	ParentID          string `json:"parent_id,omitempty"`
	ExcludeConnection string `json:"exclude_connection,omitempty"`
}

func (ns *EventNamespace) Publish(ctx context.Context, streamID, sender, body string, opts *EventPublishOptions) (*EventPublishResult, error) {
	req := map[string]any{"sender": sender, "body": body}
	if opts != nil {
		if opts.Meta != nil {
			req["meta"] = opts.Meta
		}
		if opts.ParentID != "" {
			req["parent_id"] = opts.ParentID
		}
		if opts.ExcludeConnection != "" {
			req["exclude_connection"] = opts.ExcludeConnection
		}
	}
	data, err := ns.t.request(ctx, "POST", "/streams/"+url.PathEscape(streamID)+"/events", req)
	if err != nil {
		return nil, err
	}
	var result EventPublishResult
	if err := json.Unmarshal(data, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// Delete is a chat-specific operation — event deletion.
func (ns *EventNamespace) Delete(ctx context.Context, streamID, eventID string) error {
	_, err := ns.t.request(ctx, "DELETE", "/streams/"+url.PathEscape(streamID)+"/events/"+url.PathEscape(eventID), nil)
	return err
}

// Edit is a chat-specific operation — event editing.
func (ns *EventNamespace) Edit(ctx context.Context, streamID, eventID, body string) error {
	_, err := ns.t.request(ctx, "PATCH", "/streams/"+url.PathEscape(streamID)+"/events/"+url.PathEscape(eventID), map[string]string{"body": body})
	return err
}

// GetReactions is a chat-specific operation — reaction queries.
func (ns *EventNamespace) GetReactions(ctx context.Context, streamID, eventID string) ([]ReactionSummary, error) {
	data, err := ns.t.request(ctx, "GET", "/streams/"+url.PathEscape(streamID)+"/events/"+url.PathEscape(eventID)+"/reactions", nil)
	if err != nil {
		return nil, err
	}
	var resp struct {
		Reactions []ReactionSummary `json:"reactions"`
	}
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, err
	}
	return resp.Reactions, nil
}

func (ns *EventNamespace) Trigger(ctx context.Context, streamID, event string, data any, excludeConnection *uint64) error {
	body := map[string]any{"event": event}
	if data != nil {
		body["data"] = data
	}
	if excludeConnection != nil {
		body["exclude_connection"] = *excludeConnection
	}
	_, err := ns.t.request(ctx, "POST", "/streams/"+url.PathEscape(streamID)+"/trigger", body)
	return err
}

// EventListOptions are optional parameters for listing events.
type EventListOptions struct {
	Before *uint64
	After  *uint64
	Limit  *int
	Thread *string
}

func (ns *EventNamespace) List(ctx context.Context, streamID string, opts *EventListOptions) (*EventList, error) {
	path := "/streams/" + url.PathEscape(streamID) + "/events"
	params := url.Values{}
	if opts != nil {
		if opts.Before != nil {
			params.Set("before", fmt.Sprint(*opts.Before))
		}
		if opts.After != nil {
			params.Set("after", fmt.Sprint(*opts.After))
		}
		if opts.Limit != nil {
			params.Set("limit", fmt.Sprint(*opts.Limit))
		}
		if opts.Thread != nil {
			params.Set("thread", *opts.Thread)
		}
	}
	if len(params) > 0 {
		path += "?" + params.Encode()
	}
	data, err := ns.t.request(ctx, "GET", path, nil)
	if err != nil {
		return nil, err
	}
	var list EventList
	if err := json.Unmarshal(data, &list); err != nil {
		return nil, err
	}
	return &list, nil
}

// PresenceNamespace provides presence query operations.
// This is a chat-specific namespace (conversational layer).
type PresenceNamespace struct{ t *httpTransport }

func (ns *PresenceNamespace) GetUser(ctx context.Context, userID string) (*UserPresence, error) {
	data, err := ns.t.request(ctx, "GET", "/presence/"+url.PathEscape(userID), nil)
	if err != nil {
		return nil, err
	}
	var p UserPresence
	if err := json.Unmarshal(data, &p); err != nil {
		return nil, err
	}
	return &p, nil
}

func (ns *PresenceNamespace) GetStream(ctx context.Context, streamID string) ([]MemberPresenceEntry, error) {
	data, err := ns.t.request(ctx, "GET", "/streams/"+url.PathEscape(streamID)+"/presence", nil)
	if err != nil {
		return nil, err
	}
	var resp struct {
		Members []MemberPresenceEntry `json:"members"`
	}
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, err
	}
	return resp.Members, nil
}

func (ns *PresenceNamespace) GetCursors(ctx context.Context, streamID string) ([]Cursor, error) {
	data, err := ns.t.request(ctx, "GET", "/streams/"+url.PathEscape(streamID)+"/cursors", nil)
	if err != nil {
		return nil, err
	}
	var resp struct {
		Cursors []Cursor `json:"cursors"`
	}
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, err
	}
	return resp.Cursors, nil
}

// --- Observability ---

// Connections returns active connection info from the admin endpoint.
func (h *HeraldAdmin) Connections(ctx context.Context) (json.RawMessage, error) {
	data, err := h.transport.request(ctx, "GET", "/admin/connections", nil)
	if err != nil {
		return nil, err
	}
	return json.RawMessage(data), nil
}

// AdminEventListOptions are optional parameters for listing admin events.
type AdminEventListOptions struct {
	Limit *int
}

// AdminEvents returns recent server events from the admin endpoint.
func (h *HeraldAdmin) AdminEvents(ctx context.Context, opts *AdminEventListOptions) (json.RawMessage, error) {
	path := "/admin/events"
	if opts != nil && opts.Limit != nil {
		path += fmt.Sprintf("?limit=%d", *opts.Limit)
	}
	data, err := h.transport.request(ctx, "GET", path, nil)
	if err != nil {
		return nil, err
	}
	return json.RawMessage(data), nil
}

// ErrorListOptions are optional parameters for listing errors.
type ErrorListOptions struct {
	Limit    *int
	Category *string
}

// Errors returns recent server errors from the admin endpoint.
func (h *HeraldAdmin) Errors(ctx context.Context, opts *ErrorListOptions) (json.RawMessage, error) {
	path := "/admin/errors"
	params := url.Values{}
	if opts != nil {
		if opts.Limit != nil {
			params.Set("limit", fmt.Sprint(*opts.Limit))
		}
		if opts.Category != nil {
			params.Set("category", *opts.Category)
		}
	}
	if len(params) > 0 {
		path += "?" + params.Encode()
	}
	data, err := h.transport.request(ctx, "GET", path, nil)
	if err != nil {
		return nil, err
	}
	return json.RawMessage(data), nil
}

// Stats returns server stats from the admin endpoint.
func (h *HeraldAdmin) Stats(ctx context.Context) (json.RawMessage, error) {
	data, err := h.transport.request(ctx, "GET", "/admin/stats", nil)
	if err != nil {
		return nil, err
	}
	return json.RawMessage(data), nil
}

// --- Tenant Management ---

// TenantNamespace provides tenant management operations via /admin/tenants.
type TenantNamespace struct{ t *httpTransport }

func (ns *TenantNamespace) Create(ctx context.Context, id, name string, plan *string) (*CreateTenantResponse, error) {
	body := map[string]string{"id": id, "name": name}
	if plan != nil {
		body["plan"] = *plan
	}
	data, err := ns.t.request(ctx, "POST", "/admin/tenants", body)
	if err != nil {
		return nil, err
	}
	var resp CreateTenantResponse
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, err
	}
	return &resp, nil
}

func (ns *TenantNamespace) List(ctx context.Context) ([]Tenant, error) {
	data, err := ns.t.request(ctx, "GET", "/admin/tenants", nil)
	if err != nil {
		return nil, err
	}
	var resp struct {
		Tenants []Tenant `json:"tenants"`
	}
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, err
	}
	return resp.Tenants, nil
}

func (ns *TenantNamespace) Get(ctx context.Context, id string) (*Tenant, error) {
	data, err := ns.t.request(ctx, "GET", "/admin/tenants/"+url.PathEscape(id), nil)
	if err != nil {
		return nil, err
	}
	var t Tenant
	if err := json.Unmarshal(data, &t); err != nil {
		return nil, err
	}
	return &t, nil
}

func (ns *TenantNamespace) Update(ctx context.Context, id string, name *string, plan *string, eventTTLDays *int) error {
	body := map[string]any{}
	if name != nil {
		body["name"] = *name
	}
	if plan != nil {
		body["plan"] = *plan
	}
	if eventTTLDays != nil {
		body["event_ttl_days"] = *eventTTLDays
	}
	_, err := ns.t.request(ctx, "PATCH", "/admin/tenants/"+url.PathEscape(id), body)
	return err
}

func (ns *TenantNamespace) Delete(ctx context.Context, id string) error {
	_, err := ns.t.request(ctx, "DELETE", "/admin/tenants/"+url.PathEscape(id), nil)
	return err
}

func (ns *TenantNamespace) CreateToken(ctx context.Context, tenantID string, scope *string) (string, error) {
	var body any
	if scope != nil {
		body = map[string]string{"scope": *scope}
	}
	data, err := ns.t.request(ctx, "POST", "/admin/tenants/"+url.PathEscape(tenantID)+"/tokens", body)
	if err != nil {
		return "", err
	}
	var resp struct {
		Token string `json:"token"`
	}
	if err := json.Unmarshal(data, &resp); err != nil {
		return "", err
	}
	return resp.Token, nil
}

func (ns *TenantNamespace) DeleteToken(ctx context.Context, tenantID, token string) error {
	_, err := ns.t.request(ctx, "DELETE", "/admin/tenants/"+url.PathEscape(tenantID)+"/tokens/"+url.PathEscape(token), nil)
	return err
}

func (ns *TenantNamespace) ListTokens(ctx context.Context, tenantID string) ([]string, error) {
	data, err := ns.t.request(ctx, "GET", "/admin/tenants/"+url.PathEscape(tenantID)+"/tokens", nil)
	if err != nil {
		return nil, err
	}
	var resp struct {
		Tokens []string `json:"tokens"`
	}
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, err
	}
	return resp.Tokens, nil
}

func (ns *TenantNamespace) ListStreams(ctx context.Context, tenantID string) ([]Stream, error) {
	data, err := ns.t.request(ctx, "GET", "/admin/tenants/"+url.PathEscape(tenantID)+"/streams", nil)
	if err != nil {
		return nil, err
	}
	var resp struct {
		Streams []Stream `json:"streams"`
	}
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, err
	}
	return resp.Streams, nil
}

// BlockNamespace provides user blocking operations.
// This is a chat-specific namespace (conversational layer).
type BlockNamespace struct{ t *httpTransport }

func (ns *BlockNamespace) Block(ctx context.Context, userID, blockedID string) error {
	_, err := ns.t.request(ctx, "POST", "/blocks", map[string]string{"user_id": userID, "blocked_id": blockedID})
	return err
}

func (ns *BlockNamespace) Unblock(ctx context.Context, userID, blockedID string) error {
	_, err := ns.t.request(ctx, "DELETE", "/blocks", map[string]string{"user_id": userID, "blocked_id": blockedID})
	return err
}

func (ns *BlockNamespace) List(ctx context.Context, userID string) ([]string, error) {
	data, err := ns.t.request(ctx, "GET", "/blocks/"+url.PathEscape(userID), nil)
	if err != nil {
		return nil, err
	}
	var resp struct {
		Blocked []string `json:"blocked"`
	}
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, err
	}
	return resp.Blocked, nil
}

// AuditNamespace provides audit log query operations.
type AuditNamespace struct {
	t        *httpTransport
	tenantID string
}

func (ns *AuditNamespace) basePath() string {
	return "/admin/tenants/" + url.PathEscape(ns.tenantID) + "/audit"
}

func auditParams(opts *AuditQueryOptions) url.Values {
	params := url.Values{}
	if opts == nil {
		return params
	}
	if opts.Operation != nil {
		params.Set("operation", *opts.Operation)
	}
	if opts.ResourceType != nil {
		params.Set("resource_type", *opts.ResourceType)
	}
	if opts.ResourceID != nil {
		params.Set("resource_id", *opts.ResourceID)
	}
	if opts.Actor != nil {
		params.Set("actor", *opts.Actor)
	}
	if opts.Result != nil {
		params.Set("result", *opts.Result)
	}
	if opts.Since != nil {
		params.Set("since", *opts.Since)
	}
	if opts.Until != nil {
		params.Set("until", *opts.Until)
	}
	if opts.Limit != nil {
		params.Set("limit", fmt.Sprint(*opts.Limit))
	}
	return params
}

// Query returns audit events matching the given filters.
func (ns *AuditNamespace) Query(ctx context.Context, opts *AuditQueryOptions) (*AuditQueryResult, error) {
	path := ns.basePath()
	params := auditParams(opts)
	if len(params) > 0 {
		path += "?" + params.Encode()
	}
	data, err := ns.t.request(ctx, "GET", path, nil)
	if err != nil {
		return nil, err
	}
	var result AuditQueryResult
	if err := json.Unmarshal(data, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// AuditCountOptions are optional filter parameters for audit count queries.
// Same as AuditQueryOptions but without Limit.
type AuditCountOptions struct {
	Operation    *string
	ResourceType *string
	ResourceID   *string
	Actor        *string
	Result       *string
	Since        *string
	Until        *string
}

// Count returns the number of audit events matching the given filters.
func (ns *AuditNamespace) Count(ctx context.Context, opts *AuditCountOptions) (int, error) {
	path := ns.basePath() + "/count"
	// Convert to AuditQueryOptions (without Limit) for shared param building.
	var qopts *AuditQueryOptions
	if opts != nil {
		qopts = &AuditQueryOptions{
			Operation:    opts.Operation,
			ResourceType: opts.ResourceType,
			ResourceID:   opts.ResourceID,
			Actor:        opts.Actor,
			Result:       opts.Result,
			Since:        opts.Since,
			Until:        opts.Until,
		}
	}
	params := auditParams(qopts)
	if len(params) > 0 {
		path += "?" + params.Encode()
	}
	data, err := ns.t.request(ctx, "GET", path, nil)
	if err != nil {
		return 0, err
	}
	var result AuditCountResult
	if err := json.Unmarshal(data, &result); err != nil {
		return 0, err
	}
	return result.Count, nil
}
