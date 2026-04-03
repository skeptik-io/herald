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
}

// HeraldAdmin is the Herald HTTP admin client.
type HeraldAdmin struct {
	Rooms    *RoomNamespace
	Members  *MemberNamespace
	Messages *MessageNamespace
	Presence *PresenceNamespace
	Tenants  *TenantNamespace

	transport *httpTransport
}

// New creates a new Herald admin client.
func New(opts Options) *HeraldAdmin {
	t := newTransport(opts.URL, opts.Token)
	return &HeraldAdmin{
		Rooms:     &RoomNamespace{t: t},
		Members:   &MemberNamespace{t: t},
		Messages:  &MessageNamespace{t: t},
		Presence:  &PresenceNamespace{t: t},
		Tenants:   &TenantNamespace{t: t},
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

// RoomNamespace provides room management operations.
type RoomNamespace struct{ t *httpTransport }

// RoomCreateOptions are optional parameters for room creation.
type RoomCreateOptions struct {
	Meta any `json:"meta,omitempty"`
}

func (ns *RoomNamespace) Create(ctx context.Context, id, name string, opts *RoomCreateOptions) (*Room, error) {
	body := map[string]any{"id": id, "name": name}
	if opts != nil {
		if opts.Meta != nil {
			body["meta"] = opts.Meta
		}
	}
	data, err := ns.t.request(ctx, "POST", "/rooms", body)
	if err != nil {
		return nil, err
	}
	var room Room
	if err := json.Unmarshal(data, &room); err != nil {
		return nil, err
	}
	return &room, nil
}

func (ns *RoomNamespace) Get(ctx context.Context, id string) (*Room, error) {
	data, err := ns.t.request(ctx, "GET", "/rooms/"+url.PathEscape(id), nil)
	if err != nil {
		return nil, err
	}
	var room Room
	if err := json.Unmarshal(data, &room); err != nil {
		return nil, err
	}
	return &room, nil
}

func (ns *RoomNamespace) Update(ctx context.Context, id string, name *string, meta any) error {
	body := map[string]any{}
	if name != nil {
		body["name"] = *name
	}
	if meta != nil {
		body["meta"] = meta
	}
	_, err := ns.t.request(ctx, "PATCH", "/rooms/"+url.PathEscape(id), body)
	return err
}

func (ns *RoomNamespace) List(ctx context.Context) ([]Room, error) {
	data, err := ns.t.request(ctx, "GET", "/rooms", nil)
	if err != nil {
		return nil, err
	}
	var resp struct {
		Rooms []Room `json:"rooms"`
	}
	if err := json.Unmarshal(data, &resp); err != nil {
		return nil, err
	}
	return resp.Rooms, nil
}

func (ns *RoomNamespace) Delete(ctx context.Context, id string) error {
	_, err := ns.t.request(ctx, "DELETE", "/rooms/"+url.PathEscape(id), nil)
	return err
}

// MemberNamespace provides member management operations.
type MemberNamespace struct{ t *httpTransport }

func (ns *MemberNamespace) Add(ctx context.Context, roomID, userID string, role string) (*Member, error) {
	body := map[string]string{"user_id": userID}
	if role != "" {
		body["role"] = role
	}
	data, err := ns.t.request(ctx, "POST", "/rooms/"+url.PathEscape(roomID)+"/members", body)
	if err != nil {
		return nil, err
	}
	var m Member
	if err := json.Unmarshal(data, &m); err != nil {
		return nil, err
	}
	return &m, nil
}

func (ns *MemberNamespace) List(ctx context.Context, roomID string) ([]Member, error) {
	data, err := ns.t.request(ctx, "GET", "/rooms/"+url.PathEscape(roomID)+"/members", nil)
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

func (ns *MemberNamespace) Remove(ctx context.Context, roomID, userID string) error {
	_, err := ns.t.request(ctx, "DELETE", "/rooms/"+url.PathEscape(roomID)+"/members/"+url.PathEscape(userID), nil)
	return err
}

func (ns *MemberNamespace) Update(ctx context.Context, roomID, userID, role string) error {
	_, err := ns.t.request(ctx, "PATCH", "/rooms/"+url.PathEscape(roomID)+"/members/"+url.PathEscape(userID), map[string]string{"role": role})
	return err
}

// MessageNamespace provides message operations.
type MessageNamespace struct{ t *httpTransport }

func (ns *MessageNamespace) Send(ctx context.Context, roomID, sender, body string, meta any) (*MessageSendResult, error) {
	req := map[string]any{"sender": sender, "body": body}
	if meta != nil {
		req["meta"] = meta
	}
	data, err := ns.t.request(ctx, "POST", "/rooms/"+url.PathEscape(roomID)+"/messages", req)
	if err != nil {
		return nil, err
	}
	var result MessageSendResult
	if err := json.Unmarshal(data, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// MessageListOptions are optional parameters for listing messages.
type MessageListOptions struct {
	Before *uint64
	After  *uint64
	Limit  *int
}

func (ns *MessageNamespace) List(ctx context.Context, roomID string, opts *MessageListOptions) (*MessageList, error) {
	path := "/rooms/" + url.PathEscape(roomID) + "/messages"
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
	}
	if len(params) > 0 {
		path += "?" + params.Encode()
	}
	data, err := ns.t.request(ctx, "GET", path, nil)
	if err != nil {
		return nil, err
	}
	var list MessageList
	if err := json.Unmarshal(data, &list); err != nil {
		return nil, err
	}
	return &list, nil
}

func (ns *MessageNamespace) Search(ctx context.Context, roomID, query string, limit *int) (*MessageList, error) {
	params := url.Values{"q": {query}}
	if limit != nil {
		params.Set("limit", fmt.Sprint(*limit))
	}
	path := "/rooms/" + url.PathEscape(roomID) + "/messages/search?" + params.Encode()
	data, err := ns.t.request(ctx, "GET", path, nil)
	if err != nil {
		return nil, err
	}
	var list MessageList
	if err := json.Unmarshal(data, &list); err != nil {
		return nil, err
	}
	return &list, nil
}

// PresenceNamespace provides presence query operations.
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

func (ns *PresenceNamespace) GetRoom(ctx context.Context, roomID string) ([]MemberPresenceEntry, error) {
	data, err := ns.t.request(ctx, "GET", "/rooms/"+url.PathEscape(roomID)+"/presence", nil)
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

func (ns *PresenceNamespace) GetCursors(ctx context.Context, roomID string) ([]Cursor, error) {
	data, err := ns.t.request(ctx, "GET", "/rooms/"+url.PathEscape(roomID)+"/cursors", nil)
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

// EventListOptions are optional parameters for listing events.
type EventListOptions struct {
	Limit *int
}

// Events returns recent server events from the admin endpoint.
func (h *HeraldAdmin) Events(ctx context.Context, opts *EventListOptions) (json.RawMessage, error) {
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

func (ns *TenantNamespace) Create(ctx context.Context, id, name, jwtSecret string) (*Tenant, error) {
	data, err := ns.t.request(ctx, "POST", "/admin/tenants", map[string]string{
		"id": id, "name": name, "jwt_secret": jwtSecret,
	})
	if err != nil {
		return nil, err
	}
	var t Tenant
	if err := json.Unmarshal(data, &t); err != nil {
		return nil, err
	}
	return &t, nil
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

func (ns *TenantNamespace) Update(ctx context.Context, id string, name *string, plan *string) error {
	body := map[string]any{}
	if name != nil {
		body["name"] = *name
	}
	if plan != nil {
		body["plan"] = *plan
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
