// Package web provides the HTTP handlers and templates for the
// interactive web UI.
package web

import (
	"embed"
	"encoding/json"
	"html/template"
	"io/fs"
	"log/slog"
	"net/http"
	"strings"

	"github.com/halfdane/fetching/internal/logstore"
	"github.com/halfdane/fetching/internal/progress"
	"github.com/halfdane/fetching/internal/queue"
)

//go:embed static
var staticFiles embed.FS

//go:embed templates
var templateFiles embed.FS

// Handler holds dependencies for the web UI.
type Handler struct {
	queue                  *queue.Queue
	progress               *progress.Store
	logs                   *logstore.Store
	tmpl                   *template.Template
	defaultFallbackQuality bool
}

// New creates a Handler with the given dependencies.
// ls may be nil, in which case no log streaming is provided.
// defaultFallbackQuality sets the server-wide default for the fallback-quality
// option; individual requests may override it via the fallback_quality form field.
func New(q *queue.Queue, p *progress.Store, ls *logstore.Store, defaultFallbackQuality bool) (*Handler, error) {
	tmpl, err := template.New("").ParseFS(templateFiles, "templates/*.html")
	if err != nil {
		return nil, err
	}
	return &Handler{queue: q, progress: p, logs: ls, tmpl: tmpl, defaultFallbackQuality: defaultFallbackQuality}, nil
}

// RegisterRoutes attaches all HTTP handlers to the given mux.
func (h *Handler) RegisterRoutes(mux *http.ServeMux) {
	// GET /{$} matches only the exact root "/"; the static file server below
	// catches everything else (icons, manifest.json, etc.).
	mux.HandleFunc("GET /{$}", h.handleIndex)
	mux.HandleFunc("POST /api/enqueue", h.handleEnqueue)
	mux.HandleFunc("POST /api/jobs", h.handleEnqueue)
	mux.HandleFunc("POST /api/jobs/retry", h.handleRetry)
	mux.HandleFunc("GET /api/jobs", h.handleJobs)
	mux.HandleFunc("GET /api/logs", h.handleLogs)
	mux.HandleFunc("GET /api/stream", h.handleStream)

	// Serve embedded static assets (SVGs, PNGs, manifest.json) at root.
	staticFS, _ := fs.Sub(staticFiles, "static")
	mux.Handle("GET /", http.FileServerFS(staticFS))
}

func (h *Handler) handleIndex(w http.ResponseWriter, r *http.Request) {
	collections := h.progress.Snapshot()
	data := struct {
		Collections []progress.CollectionView
	}{Collections: collections}

	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	if err := h.tmpl.ExecuteTemplate(w, "index", data); err != nil {
		slog.Error("template error", "err", err)
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

	fallbackQuality := h.defaultFallbackQuality || r.FormValue("fallback_quality") == "on"
	jobs, err := h.queue.Enqueue(queue.EnqueueOptions{FallbackQuality: fallbackQuality}, uris...)
	if err != nil {
		slog.Error("enqueue error", "err", err)
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
	if err := r.ParseForm(); err != nil {
		http.Error(w, "bad request", http.StatusBadRequest)
		return
	}

	uri := strings.TrimSpace(r.FormValue("uri"))
	if uri == "" {
		http.Error(w, "uri is required", http.StatusBadRequest)
		return
	}

	fallbackQuality := h.defaultFallbackQuality || r.FormValue("fallback_quality") == "on"
	result, err := h.queue.Retry(queue.EnqueueOptions{FallbackQuality: fallbackQuality}, uri)
	if err != nil {
		slog.Error("retry error", "err", err)
		http.Error(w, "failed to retry", http.StatusInternalServerError)
		return
	}

	// Remove stale progress entries for the deleted jobs, then register the new one.
	h.progress.Remove(result.DeletedIDs...)
	h.progress.UpsertSubmitted(result.NewJob.ID, result.NewJob.SpotifyURI)

	if strings.Contains(r.Header.Get("Accept"), "application/json") {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"jobId":      result.NewJob.ID,
			"sourceUri":  result.NewJob.SpotifyURI,
			"acceptedAt": result.NewJob.CreatedAt,
		})
		return
	}

	http.Redirect(w, r, "/", http.StatusSeeOther)
}

func (h *Handler) handleJobs(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(map[string]any{
		"collections": h.progress.Snapshot(),
	})
}

func (h *Handler) handleLogs(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	var entries []logstore.Entry
	if h.logs != nil {
		entries = h.logs.Recent(maxEntries)
	}
	if entries == nil {
		entries = []logstore.Entry{}
	}
	_ = json.NewEncoder(w).Encode(map[string]any{"entries": entries})
}

const maxEntries = 300

func (h *Handler) handleStream(w http.ResponseWriter, r *http.Request) {
	flusher, ok := w.(http.Flusher)
	if !ok {
		http.Error(w, "streaming unsupported", http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")

	progressCh, unsubProgress := h.progress.Subscribe()
	defer unsubProgress()

	var logCh <-chan logstore.Entry
	if h.logs != nil {
		var unsubLog func()
		logCh, unsubLog = h.logs.Subscribe()
		defer unsubLog()
	}

	ctx := r.Context()
	for {
		select {
		case <-ctx.Done():
			return
		case event, ok := <-progressCh:
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
		case entry, ok := <-logCh:
			if !ok {
				return
			}
			payload, err := json.Marshal(entry)
			if err != nil {
				continue
			}
			_, _ = w.Write([]byte("event: log\n"))
			_, _ = w.Write([]byte("data: " + string(payload) + "\n\n"))
			flusher.Flush()
		}
	}
}
