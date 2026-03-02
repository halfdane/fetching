// Package web provides the HTTP handlers and templates for the
// interactive web UI.
package web

import (
	"encoding/json"
	"html/template"
	"log"
	"net/http"
	"strings"

	"github.com/halfdane/fetching/internal/queue"
)

// Handler holds dependencies for the web UI.
type Handler struct {
	queue *queue.Queue
	tmpl  *template.Template
}

// New creates a Handler with the given queue.
func New(q *queue.Queue) (*Handler, error) {
	tmpl, err := template.New("").Parse(indexTemplate + jobsPartial)
	if err != nil {
		return nil, err
	}
	return &Handler{queue: q, tmpl: tmpl}, nil
}

// RegisterRoutes attaches all HTTP handlers to the given mux.
func (h *Handler) RegisterRoutes(mux *http.ServeMux) {
	mux.HandleFunc("GET /", h.handleIndex)
	mux.HandleFunc("POST /api/enqueue", h.handleEnqueue)
	mux.HandleFunc("GET /api/jobs", h.handleJobs)
}

func (h *Handler) handleIndex(w http.ResponseWriter, r *http.Request) {
	jobs, err := h.queue.List()
	if err != nil {
		http.Error(w, "failed to list jobs", http.StatusInternalServerError)
		return
	}

	data := struct {
		Jobs []*queue.Job
	}{Jobs: jobs}

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

	// Support multiple URIs separated by newlines or spaces
	var uris []string
	for _, line := range strings.Split(input, "\n") {
		line = strings.TrimSpace(line)
		if line != "" {
			uris = append(uris, line)
		}
	}

	if len(uris) == 0 {
		http.Error(w, "no valid URIs provided", http.StatusBadRequest)
		return
	}

	jobs, err := h.queue.Enqueue(uris...)
	if err != nil {
		log.Printf("enqueue error: %v", err)
		http.Error(w, "failed to enqueue", http.StatusInternalServerError)
		return
	}

	// If the request accepts JSON, return JSON
	if strings.Contains(r.Header.Get("Accept"), "application/json") {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(jobs)
		return
	}

	// Otherwise redirect back to the index
	http.Redirect(w, r, "/", http.StatusSeeOther)
}

func (h *Handler) handleJobs(w http.ResponseWriter, r *http.Request) {
	jobs, err := h.queue.List()
	if err != nil {
		http.Error(w, "failed to list jobs", http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(jobs)
}
