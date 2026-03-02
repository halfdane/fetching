package web

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/halfdane/fetching/internal/progress"
	"github.com/halfdane/fetching/internal/queue"
)

// newTestHandler creates a Handler backed by an in-memory SQLite queue
// and a fresh progress store.
func newTestHandler(t *testing.T) (*Handler, *queue.Queue, *progress.Store) {
	t.Helper()
	q, err := queue.New(":memory:")
	if err != nil {
		t.Fatalf("queue.New: %v", err)
	}
	t.Cleanup(func() { q.Close() })

	p := progress.NewStore()
	h, err := New(q, p)
	if err != nil {
		t.Fatalf("web.New: %v", err)
	}
	return h, q, p
}

func mux(h *Handler) *http.ServeMux {
	m := http.NewServeMux()
	h.RegisterRoutes(m)
	return m
}

// ---- GET / ----

func TestIndexReturnsHTML(t *testing.T) {
	h, _, _ := newTestHandler(t)
	r := httptest.NewRequest("GET", "/", nil)
	w := httptest.NewRecorder()
	mux(h).ServeHTTP(w, r)

	if w.Code != 200 {
		t.Fatalf("status = %d, want 200", w.Code)
	}
	ct := w.Header().Get("Content-Type")
	if !strings.Contains(ct, "text/html") {
		t.Errorf("Content-Type = %q, want text/html", ct)
	}
	if !strings.Contains(w.Body.String(), "fetching") {
		t.Error("body does not contain app title")
	}
}

// ---- POST /api/jobs ----

func TestEnqueueSingleURI(t *testing.T) {
	h, _, p := newTestHandler(t)

	body := strings.NewReader("uri=spotify:album:abc")
	r := httptest.NewRequest("POST", "/api/jobs", body)
	r.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	r.Header.Set("Accept", "application/json")
	w := httptest.NewRecorder()
	mux(h).ServeHTTP(w, r)

	if w.Code != 200 {
		t.Fatalf("status = %d, want 200; body = %s", w.Code, w.Body.String())
	}

	var resp map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &resp); err != nil {
		t.Fatalf("invalid JSON: %v", err)
	}
	if resp["sourceUri"] != "spotify:album:abc" {
		t.Errorf("sourceUri = %v, want spotify:album:abc", resp["sourceUri"])
	}

	// Progress store should reflect the new job.
	snap := p.Snapshot()
	if len(snap) != 1 {
		t.Fatalf("progress snapshot has %d collections, want 1", len(snap))
	}
}

func TestEnqueueRejectsMultipleURIs(t *testing.T) {
	h, _, _ := newTestHandler(t)

	body := strings.NewReader("uri=spotify:album:a\nspotify:album:b")
	r := httptest.NewRequest("POST", "/api/jobs", body)
	r.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	w := httptest.NewRecorder()
	mux(h).ServeHTTP(w, r)

	if w.Code != 400 {
		t.Errorf("status = %d, want 400 for multiple URIs", w.Code)
	}
}

func TestEnqueueRejectsEmpty(t *testing.T) {
	h, _, _ := newTestHandler(t)

	body := strings.NewReader("uri=")
	r := httptest.NewRequest("POST", "/api/jobs", body)
	r.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	w := httptest.NewRecorder()
	mux(h).ServeHTTP(w, r)

	if w.Code != 400 {
		t.Errorf("status = %d, want 400 for empty uri", w.Code)
	}
}

func TestEnqueueRedirectsForBrowser(t *testing.T) {
	h, _, _ := newTestHandler(t)

	body := strings.NewReader("uri=spotify:album:abc")
	r := httptest.NewRequest("POST", "/api/jobs", body)
	r.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	// No Accept: application/json → should redirect
	w := httptest.NewRecorder()
	mux(h).ServeHTTP(w, r)

	if w.Code != http.StatusSeeOther {
		t.Errorf("status = %d, want %d", w.Code, http.StatusSeeOther)
	}
}

// ---- POST /api/jobs/retry ----

func TestRetryEndpointEnqueues(t *testing.T) {
	h, _, p := newTestHandler(t)

	body := strings.NewReader("uri=spotify:album:abc")
	r := httptest.NewRequest("POST", "/api/jobs/retry", body)
	r.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	r.Header.Set("Accept", "application/json")
	w := httptest.NewRecorder()
	mux(h).ServeHTTP(w, r)

	if w.Code != 200 {
		t.Fatalf("status = %d, want 200; body = %s", w.Code, w.Body.String())
	}

	snap := p.Snapshot()
	if len(snap) != 1 {
		t.Fatalf("progress snapshot has %d collections, want 1", len(snap))
	}
}

// ---- GET /api/jobs ----

func TestGetJobsReturnsSnapshot(t *testing.T) {
	h, _, p := newTestHandler(t)
	p.UpsertSubmitted(99, "spotify:album:xyz")

	r := httptest.NewRequest("GET", "/api/jobs", nil)
	w := httptest.NewRecorder()
	mux(h).ServeHTTP(w, r)

	if w.Code != 200 {
		t.Fatalf("status = %d, want 200", w.Code)
	}

	var resp struct {
		Collections []progress.CollectionView `json:"collections"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &resp); err != nil {
		t.Fatalf("invalid JSON: %v", err)
	}
	if len(resp.Collections) != 1 {
		t.Fatalf("expected 1 collection, got %d", len(resp.Collections))
	}
	if resp.Collections[0].SourceURI != "spotify:album:xyz" {
		t.Errorf("SourceURI = %q, want %q", resp.Collections[0].SourceURI, "spotify:album:xyz")
	}
}

// ---- GET /api/stream ----

func TestSSEStreamSendsInitialSnapshot(t *testing.T) {
	h, _, p := newTestHandler(t)
	p.UpsertSubmitted(1, "spotify:album:abc")

	m := mux(h)
	srv := httptest.NewServer(m)
	defer srv.Close()

	resp, err := http.Get(srv.URL + "/api/stream")
	if err != nil {
		t.Fatalf("GET /api/stream: %v", err)
	}
	defer resp.Body.Close()

	if resp.Header.Get("Content-Type") != "text/event-stream" {
		t.Errorf("Content-Type = %q, want text/event-stream", resp.Header.Get("Content-Type"))
	}

	// Read one SSE frame: "event: snapshot\ndata: {...}\n\n"
	buf := make([]byte, 8192)
	n, err := resp.Body.Read(buf)
	if err != nil {
		t.Fatalf("read SSE: %v", err)
	}
	frame := string(buf[:n])

	if !strings.Contains(frame, "event: snapshot") {
		t.Error("expected 'event: snapshot' in SSE frame")
	}
	if !strings.Contains(frame, "spotify:album:abc") {
		t.Error("expected source URI in SSE data")
	}
}

// ---- Legacy /api/enqueue ----

func TestLegacyEnqueueRoute(t *testing.T) {
	h, _, _ := newTestHandler(t)

	body := strings.NewReader("uri=spotify:album:abc")
	r := httptest.NewRequest("POST", "/api/enqueue", body)
	r.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	r.Header.Set("Accept", "application/json")
	w := httptest.NewRecorder()
	mux(h).ServeHTTP(w, r)

	if w.Code != 200 {
		t.Fatalf("status = %d, want 200", w.Code)
	}
}
