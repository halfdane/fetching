// Package web provides the HTTP handlers and templates for the
// interactive web UI.
package web

import (
	"encoding/json"
	"html/template"
	"log"
	"net/http"
	"strings"

	"github.com/halfdane/fetching/internal/progress"
	"github.com/halfdane/fetching/internal/queue"
)

// Handler holds dependencies for the web UI.
type Handler struct {
	queue    *queue.Queue
	progress *progress.Store
	tmpl     *template.Template
}

// New creates a Handler with the given queue.
func New(q *queue.Queue, p *progress.Store) (*Handler, error) {
	tmpl, err := template.New("").Parse(indexTemplate + jobsPartial)
	if err != nil {
		return nil, err
	}
	return &Handler{queue: q, progress: p, tmpl: tmpl}, nil
}

// RegisterRoutes attaches all HTTP handlers to the given mux.
func (h *Handler) RegisterRoutes(mux *http.ServeMux) {
	mux.HandleFunc("GET /", h.handleIndex)
	mux.HandleFunc("POST /api/enqueue", h.handleEnqueue)
	mux.HandleFunc("POST /api/jobs", h.handleEnqueue)
	mux.HandleFunc("POST /api/jobs/retry", h.handleRetry)
	mux.HandleFunc("GET /api/jobs", h.handleJobs)
	mux.HandleFunc("GET /api/stream", h.handleStream)
}

func (h *Handler) handleIndex(w http.ResponseWriter, r *http.Request) {
	collections := h.progress.Snapshot()
	data := struct {
		Collections []progress.CollectionView
	}{Collections: collections}

	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	if err := h.tmpl.ExecuteTemplate(w, "index", data); err != nil {
		log.Printf("template error: %v", err)
	}
}

func (h *Handler) handleEnqueue(w http.ResponseWriter, r *http.Request) {
	if err := r.ParseForm(); err != nil {
		http.Error(w, "bad request", http.StatusBadRequest)
		return
	}

	input := r.FormValue("uri")
	if input == "" {
		http.Error(w, "uri is required", http.StatusBadRequest)
		return
	}

	// One URL per submit by design.
	var uris []string
	for _, line := range strings.Split(input, "\n") {
		line = strings.TrimSpace(line)
		if line != "" {
			uris = append(uris, line)
		}
	}
	if len(uris) != 1 {
		http.Error(w, "submit exactly one URL or URI", http.StatusBadRequest)
		return
	}

	jobs, err := h.queue.Enqueue(uris...)
	if err != nil {
		log.Printf("enqueue error: %v", err)
		http.Error(w, "failed to enqueue", http.StatusInternalServerError)
		return
	}

	for _, j := range jobs {
		h.progress.UpsertSubmitted(j.ID, j.SpotifyURI)
	}

	// If the request accepts JSON, return JSON.
	if strings.Contains(r.Header.Get("Accept"), "application/json") {
		w.Header().Set("Content-Type", "application/json")
		if len(jobs) > 0 {
			_ = json.NewEncoder(w).Encode(map[string]any{
				"jobId":      jobs[0].ID,
				"sourceUri":  jobs[0].SpotifyURI,
				"acceptedAt": jobs[0].CreatedAt,
			})
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"ok": true})
		return
	}

	// Otherwise redirect back to the index
	http.Redirect(w, r, "/", http.StatusSeeOther)
}

func (h *Handler) handleRetry(w http.ResponseWriter, r *http.Request) {
	h.handleEnqueue(w, r)
}

func (h *Handler) handleJobs(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(map[string]any{
		"collections": h.progress.Snapshot(),
	})
}

func (h *Handler) handleStream(w http.ResponseWriter, r *http.Request) {
	flusher, ok := w.(http.Flusher)
	if !ok {
		http.Error(w, "streaming unsupported", http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")

	ch, unsubscribe := h.progress.Subscribe()
	defer unsubscribe()

	ctx := r.Context()
	for {
		select {
		case <-ctx.Done():
			return
		case event, ok := <-ch:
			if !ok {
				return
			}
			payload, err := json.Marshal(event)
			if err != nil {
				continue
			}
			_, _ = w.Write([]byte("event: " + event.Type + "\n"))
			_, _ = w.Write([]byte("data: " + string(payload) + "\n\n"))
			flusher.Flush()
		}
	}
}
