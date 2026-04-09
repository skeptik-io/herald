package contract_test

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	herald "github.com/nicklucas/herald-admin-go"
)

// Spec types matching spec.json structure.

type FieldSpec map[string]string

type ExpectSpec struct {
	Void             bool                   `json:"void"`
	Fields           FieldSpec              `json:"fields"`
	Values           map[string]interface{} `json:"values"`
	Type             string                 `json:"type"`
	Length           *int                   `json:"length"`
	MinLength        *int                   `json:"min_length"`
	Contains         *string                `json:"contains"`
	ItemFields       FieldSpec              `json:"item_fields"`
	EventsLength     *int                   `json:"events_length"`
	EventsMinLength  *int                   `json:"events_min_length"`
	EventFields      FieldSpec              `json:"event_fields"`
	FirstEventValues map[string]interface{} `json:"first_event_values"`
	FirstEventFields FieldSpec              `json:"first_event_fields"`
}

type SaveSpec map[string]string

type TestCase struct {
	ID        string                 `json:"id"`
	Operation string                 `json:"operation"`
	Input     map[string]interface{} `json:"input"`
	Expect    ExpectSpec             `json:"expect"`
	Save      SaveSpec               `json:"save"`
}

type TestGroup struct {
	Name  string     `json:"name"`
	Auth  string     `json:"auth"`
	Cases []TestCase `json:"cases"`
}

type ContractSpec struct {
	Version int         `json:"version"`
	Groups  []TestGroup `json:"groups"`
}

func loadSpec(t *testing.T) ContractSpec {
	t.Helper()
	specPath := os.Getenv("HERALD_SPEC_PATH")
	if specPath == "" {
		specPath = filepath.Join("..", "spec", "spec.json")
	}
	data, err := os.ReadFile(specPath)
	if err != nil {
		t.Fatalf("load spec: %v", err)
	}
	var spec ContractSpec
	if err := json.Unmarshal(data, &spec); err != nil {
		t.Fatalf("parse spec: %v", err)
	}
	return spec
}

func resolveString(val interface{}, saved map[string]interface{}) interface{} {
	s, ok := val.(string)
	if !ok || len(s) == 0 || s[0] != '$' {
		return val
	}
	ref := s[1:]
	if ref == "event_seq_minus_1" {
		seq, _ := saved["event_seq"].(float64)
		return seq - 1
	}
	return saved[ref]
}

func resolveInput(input map[string]interface{}, saved map[string]interface{}) map[string]interface{} {
	out := make(map[string]interface{}, len(input))
	for k, v := range input {
		out[k] = resolveString(v, saved)
	}
	return out
}

func getString(m map[string]interface{}, key string) string {
	v, _ := m[key].(string)
	return v
}

func getFloat(m map[string]interface{}, key string) float64 {
	v, _ := m[key].(float64)
	return v
}

func getInt(m map[string]interface{}, key string) int {
	return int(getFloat(m, key))
}

func getBool(m map[string]interface{}, key string) bool {
	v, _ := m[key].(bool)
	return v
}

type runner struct {
	client *herald.HeraldAdmin
	saved  map[string]interface{}
	passed int
	failed int
}

func (r *runner) execute(ctx context.Context, t *testing.T, tc TestCase) interface{} {
	t.Helper()
	input := resolveInput(tc.Input, r.saved)

	switch tc.Operation {
	case "health":
		h, err := r.client.Health(ctx)
		if err != nil {
			return err
		}
		return map[string]interface{}{
			"status": h.Status, "connections": float64(h.Connections),
			"streams": float64(h.Streams), "uptime_secs": float64(h.UptimeSecs),
		}

	case "streams.create":
		var opts *herald.StreamCreateOptions
		if input["meta"] != nil || getBool(input, "public") {
			opts = &herald.StreamCreateOptions{Meta: input["meta"], Public: getBool(input, "public")}
		}
		s, err := r.client.Streams.Create(ctx, getString(input, "id"), getString(input, "name"), opts)
		if err != nil {
			return err
		}
		return streamToMap(s)

	case "streams.get":
		s, err := r.client.Streams.Get(ctx, getString(input, "id"))
		if err != nil {
			return err
		}
		return streamToMap(s)

	case "streams.list":
		list, err := r.client.Streams.List(ctx)
		if err != nil {
			return err
		}
		arr := make([]interface{}, len(list))
		for i, s := range list {
			arr[i] = streamToMap(&s)
		}
		return arr

	case "streams.update":
		name, _ := input["name"].(string)
		var namePtr *string
		if name != "" {
			namePtr = &name
		}
		var archivedPtr *bool
		if v, ok := input["archived"]; ok {
			b := v.(bool)
			archivedPtr = &b
		}
		if err := r.client.Streams.Update(ctx, getString(input, "id"), namePtr, input["meta"], archivedPtr); err != nil {
			return err
		}
		return nil

	case "streams.delete":
		if err := r.client.Streams.Delete(ctx, getString(input, "id")); err != nil {
			return err
		}
		return nil

	case "members.add":
		m, err := r.client.Members.Add(ctx, getString(input, "stream_id"), getString(input, "user_id"), getString(input, "role"))
		if err != nil {
			return err
		}
		return memberToMap(m)

	case "members.list":
		list, err := r.client.Members.List(ctx, getString(input, "stream_id"))
		if err != nil {
			return err
		}
		arr := make([]interface{}, len(list))
		for i, m := range list {
			arr[i] = memberToMap(&m)
		}
		return arr

	case "members.update":
		if err := r.client.Members.Update(ctx, getString(input, "stream_id"), getString(input, "user_id"), getString(input, "role")); err != nil {
			return err
		}
		return nil

	case "members.remove":
		if err := r.client.Members.Remove(ctx, getString(input, "stream_id"), getString(input, "user_id")); err != nil {
			return err
		}
		return nil

	case "events.publish":
		var opts *herald.EventPublishOptions
		if input["meta"] != nil || getString(input, "parent_id") != "" {
			opts = &herald.EventPublishOptions{
				Meta:     input["meta"],
				ParentID: getString(input, "parent_id"),
			}
		}
		res, err := r.client.Events.Publish(ctx, getString(input, "stream_id"), getString(input, "sender"), getString(input, "body"), opts)
		if err != nil {
			return err
		}
		return map[string]interface{}{
			"id": res.ID, "seq": float64(res.Seq), "sent_at": float64(res.SentAt),
		}

	case "events.list":
		var opts *herald.EventListOptions
		if len(input) > 1 {
			opts = &herald.EventListOptions{}
			if v, ok := input["before"]; ok {
				u := uint64(v.(float64))
				opts.Before = &u
			}
			if v, ok := input["after"]; ok {
				u := uint64(v.(float64))
				opts.After = &u
			}
			if v, ok := input["limit"]; ok {
				i := int(v.(float64))
				opts.Limit = &i
			}
			if v, ok := input["thread"]; ok {
				s := v.(string)
				opts.Thread = &s
			}
		}
		el, err := r.client.Events.List(ctx, getString(input, "stream_id"), opts)
		if err != nil {
			return err
		}
		events := make([]interface{}, len(el.Events))
		for i, e := range el.Events {
			events[i] = eventToMap(&e)
		}
		return map[string]interface{}{"events": events, "has_more": el.HasMore}

	case "events.edit":
		if err := r.client.Events.Edit(ctx, getString(input, "stream_id"), getString(input, "event_id"), getString(input, "body")); err != nil {
			return err
		}
		return nil

	case "events.delete":
		if err := r.client.Events.Delete(ctx, getString(input, "stream_id"), getString(input, "event_id")); err != nil {
			return err
		}
		return nil

	case "events.getReactions":
		reactions, err := r.client.Events.GetReactions(ctx, getString(input, "stream_id"), getString(input, "event_id"))
		if err != nil {
			return err
		}
		arr := make([]interface{}, len(reactions))
		for i, rx := range reactions {
			arr[i] = map[string]interface{}{"emoji": rx.Emoji, "count": float64(rx.Count), "users": rx.Users}
		}
		return arr

	case "events.trigger":
		if err := r.client.Events.Trigger(ctx, getString(input, "stream_id"), getString(input, "event"), input["data"], nil); err != nil {
			return err
		}
		return nil

	case "presence.getUser":
		p, err := r.client.Chat.Presence.GetUser(ctx, getString(input, "user_id"))
		if err != nil {
			return err
		}
		return map[string]interface{}{"user_id": p.UserID, "status": p.Status, "connections": float64(p.Connections)}

	case "presence.getStream":
		members, err := r.client.Chat.Presence.GetStream(ctx, getString(input, "stream_id"))
		if err != nil {
			return err
		}
		arr := make([]interface{}, len(members))
		for i, m := range members {
			arr[i] = map[string]interface{}{"user_id": m.UserID, "status": m.Status}
		}
		return arr

	case "presence.getCursors":
		cursors, err := r.client.Chat.Presence.GetCursors(ctx, getString(input, "stream_id"))
		if err != nil {
			return err
		}
		arr := make([]interface{}, len(cursors))
		for i, c := range cursors {
			arr[i] = map[string]interface{}{"user_id": c.UserID, "seq": float64(c.Seq)}
		}
		return arr

	case "blocks.block":
		if err := r.client.Chat.Blocks.Block(ctx, getString(input, "user_id"), getString(input, "blocked_id")); err != nil {
			return err
		}
		return nil

	case "blocks.unblock":
		if err := r.client.Chat.Blocks.Unblock(ctx, getString(input, "user_id"), getString(input, "blocked_id")); err != nil {
			return err
		}
		return nil

	case "blocks.list":
		blocked, err := r.client.Chat.Blocks.List(ctx, getString(input, "user_id"))
		if err != nil {
			return err
		}
		arr := make([]interface{}, len(blocked))
		for i, b := range blocked {
			arr[i] = b
		}
		return arr

	default:
		return fmt.Errorf("unknown operation: %s", tc.Operation)
	}
}

func streamToMap(s *herald.Stream) map[string]interface{} {
	m := map[string]interface{}{
		"id": s.ID, "name": s.Name, "public": s.Public,
		"archived": s.Archived, "created_at": float64(s.CreatedAt),
	}
	if s.Meta != nil {
		m["meta"] = s.Meta
	}
	return m
}

func memberToMap(m *herald.Member) map[string]interface{} {
	return map[string]interface{}{
		"stream_id": m.StreamID, "user_id": m.UserID,
		"role": m.Role, "joined_at": float64(m.JoinedAt),
	}
}

func eventToMap(e *herald.Event) map[string]interface{} {
	m := map[string]interface{}{
		"id": e.ID, "stream": e.Stream, "seq": float64(e.Seq),
		"sender": e.Sender, "body": e.Body, "sent_at": float64(e.SentAt),
	}
	if e.Meta != nil {
		m["meta"] = e.Meta
	}
	if e.ParentID != "" {
		m["parent_id"] = e.ParentID
	}
	if e.EditedAt != nil {
		m["edited_at"] = float64(*e.EditedAt)
	}
	return m
}

func typeOf(val interface{}) string {
	if val == nil {
		return "null"
	}
	switch val.(type) {
	case string:
		return "string"
	case float64:
		return "number"
	case bool:
		return "boolean"
	case []interface{}:
		return "array"
	case map[string]interface{}:
		return "object"
	default:
		return fmt.Sprintf("%T", val)
	}
}

func validateFields(obj map[string]interface{}, fields FieldSpec) []string {
	var errs []string
	for key, expectedType := range fields {
		val, ok := obj[key]
		if !ok {
			errs = append(errs, fmt.Sprintf("missing field: %s", key))
			continue
		}
		if expectedType == "any" {
			continue
		}
		actual := typeOf(val)
		if expectedType == "object" && (actual == "object" || actual == "null") {
			continue
		}
		if actual != expectedType {
			errs = append(errs, fmt.Sprintf("%s: expected type %s, got %s", key, expectedType, actual))
		}
	}
	return errs
}

func validateExpect(result interface{}, expect ExpectSpec, saved map[string]interface{}) []string {
	var errs []string

	if expect.Void {
		return errs
	}

	if expect.Type == "array" {
		arr, ok := result.([]interface{})
		if !ok {
			return append(errs, fmt.Sprintf("expected array, got %s", typeOf(result)))
		}
		if expect.Length != nil && len(arr) != *expect.Length {
			errs = append(errs, fmt.Sprintf("expected array length %d, got %d", *expect.Length, len(arr)))
		}
		if expect.MinLength != nil && len(arr) < *expect.MinLength {
			errs = append(errs, fmt.Sprintf("expected array min length %d, got %d", *expect.MinLength, len(arr)))
		}
		if expect.Contains != nil {
			found := false
			for _, item := range arr {
				if s, ok := item.(string); ok && s == *expect.Contains {
					found = true
					break
				}
			}
			if !found {
				errs = append(errs, fmt.Sprintf("expected array to contain %q", *expect.Contains))
			}
		}
		if expect.ItemFields != nil && len(arr) > 0 {
			if m, ok := arr[0].(map[string]interface{}); ok {
				errs = append(errs, validateFields(m, expect.ItemFields)...)
			}
		}
		return errs
	}

	obj, _ := result.(map[string]interface{})
	if obj == nil && expect.Fields != nil {
		return append(errs, "expected object, got nil")
	}

	if expect.Fields != nil {
		errs = append(errs, validateFields(obj, expect.Fields)...)
	}

	if expect.Values != nil {
		for key, expected := range expect.Values {
			resolved := resolveString(expected, saved)
			actual := obj[key]
			if !valuesEqual(actual, resolved) {
				errs = append(errs, fmt.Sprintf("%s: expected %v, got %v", key, resolved, actual))
			}
		}
	}

	if expect.EventsLength != nil {
		events, _ := obj["events"].([]interface{})
		if events == nil {
			errs = append(errs, "expected events array")
		} else if len(events) != *expect.EventsLength {
			errs = append(errs, fmt.Sprintf("expected events length %d, got %d", *expect.EventsLength, len(events)))
		}
	}

	if expect.EventsMinLength != nil {
		events, _ := obj["events"].([]interface{})
		if events == nil {
			errs = append(errs, "expected events array")
		} else if len(events) < *expect.EventsMinLength {
			errs = append(errs, fmt.Sprintf("expected events min length %d, got %d", *expect.EventsMinLength, len(events)))
		}
	}

	if expect.EventFields != nil {
		events, _ := obj["events"].([]interface{})
		if len(events) > 0 {
			if m, ok := events[0].(map[string]interface{}); ok {
				errs = append(errs, validateFields(m, expect.EventFields)...)
			}
		}
	}

	if expect.FirstEventValues != nil {
		events, _ := obj["events"].([]interface{})
		if len(events) > 0 {
			if m, ok := events[0].(map[string]interface{}); ok {
				for key, expected := range expect.FirstEventValues {
					resolved := resolveString(expected, saved)
					if !valuesEqual(m[key], resolved) {
						errs = append(errs, fmt.Sprintf("first event %s: expected %v, got %v", key, resolved, m[key]))
					}
				}
			}
		}
	}

	if expect.FirstEventFields != nil {
		events, _ := obj["events"].([]interface{})
		if len(events) > 0 {
			if m, ok := events[0].(map[string]interface{}); ok {
				errs = append(errs, validateFields(m, expect.FirstEventFields)...)
			}
		}
	}

	return errs
}

func valuesEqual(a, b interface{}) bool {
	// Handle numeric comparison (JSON numbers are float64)
	if af, ok := a.(float64); ok {
		if bf, ok := b.(float64); ok {
			return af == bf
		}
	}
	return fmt.Sprintf("%v", a) == fmt.Sprintf("%v", b)
}

func TestContract(t *testing.T) {
	spec := loadSpec(t)

	url := os.Getenv("HERALD_URL")
	apiToken := os.Getenv("HERALD_API_TOKEN")

	if url == "" || apiToken == "" {
		t.Skip("HERALD_URL and HERALD_API_TOKEN required")
	}

	r := &runner{
		client: herald.New(herald.Options{URL: url, Token: apiToken, TenantID: "default"}),
		saved:  make(map[string]interface{}),
	}

	ctx := context.Background()

	for _, group := range spec.Groups {
		for _, tc := range group.Cases {
			t.Run(tc.ID, func(t *testing.T) {
				result := r.execute(ctx, t, tc)

				if err, ok := result.(error); ok {
					t.Fatalf("operation failed: %v", err)
				}

				if tc.Save != nil {
					if m, ok := result.(map[string]interface{}); ok {
						for alias, field := range tc.Save {
							r.saved[alias] = m[field]
						}
					}
				}

				errs := validateExpect(result, tc.Expect, r.saved)
				for _, e := range errs {
					t.Errorf("%s", e)
				}
			})
		}
	}

	fmt.Printf("\n  Go: %d passed, %d failed\n", r.passed, r.failed)
}
