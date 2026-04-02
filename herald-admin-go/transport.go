package herald

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
)

type httpTransport struct {
	baseURL string
	token   string
	client  *http.Client
}

func newTransport(baseURL, token string) *httpTransport {
	return &httpTransport{
		baseURL: strings.TrimRight(baseURL, "/"),
		token:   token,
		client:  &http.Client{},
	}
}

func (t *httpTransport) request(ctx context.Context, method, path string, body any) (json.RawMessage, error) {
	var bodyReader io.Reader
	if body != nil {
		b, err := json.Marshal(body)
		if err != nil {
			return nil, fmt.Errorf("marshal body: %w", err)
		}
		bodyReader = bytes.NewReader(b)
	}

	req, err := http.NewRequestWithContext(ctx, method, t.baseURL+path, bodyReader)
	if err != nil {
		return nil, fmt.Errorf("create request: %w", err)
	}

	req.Header.Set("Authorization", "Bearer "+t.token)
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}

	resp, err := t.client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("http request: %w", err)
	}
	defer resp.Body.Close()

	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("read response: %w", err)
	}

	if resp.StatusCode >= 400 {
		code := "INTERNAL"
		msg := string(data)
		var errResp struct {
			Error string `json:"error"`
		}
		if json.Unmarshal(data, &errResp) == nil && errResp.Error != "" {
			msg = errResp.Error
		}
		switch {
		case resp.StatusCode == 401:
			code = "UNAUTHORIZED"
		case resp.StatusCode == 404:
			code = "NOT_FOUND"
		case resp.StatusCode == 429:
			code = "RATE_LIMITED"
		}
		return nil, &HeraldError{Code: code, Message: msg, Status: resp.StatusCode}
	}

	if resp.StatusCode == 204 {
		return nil, nil
	}

	return json.RawMessage(data), nil
}
