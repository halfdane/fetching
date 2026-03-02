// Package logstore provides an slog.Handler that tees log records to a
// wrapped handler (e.g. stderr) and stores the most recent entries in an
// in-memory ring buffer for live streaming to the web UI.
package logstore

import (
	"context"
	"io"
	"log/slog"
	"sync"
	"time"
)

const maxEntries = 300

// Entry is a single captured log record for the web UI.
type Entry struct {
	Time    time.Time         `json:"time"`
	Level   string            `json:"level"`
	Message string            `json:"message"`
	Attrs   map[string]string `json:"attrs,omitempty"`
}

// Store buffers recent log entries and fans them out to SSE subscribers.
type Store struct {
	mu      sync.Mutex
	entries []Entry
	subs    map[chan Entry]struct{}
}

// New creates a new Store.
func New() *Store {
	return &Store{
		subs: make(map[chan Entry]struct{}),
	}
}

// Handler returns an slog.Handler that writes to w and also records entries
// in this store. Pass os.Stderr for w to get human-readable output on stderr.
func (s *Store) Handler(w io.Writer) slog.Handler {
	text := slog.NewTextHandler(w, &slog.HandlerOptions{Level: slog.LevelInfo})
	return &teeHandler{store: s, wrapped: text}
}

// Recent returns a copy of the last n entries (or fewer if the buffer is smaller).
func (s *Store) Recent(n int) []Entry {
	s.mu.Lock()
	defer s.mu.Unlock()
	if n <= 0 || len(s.entries) == 0 {
		return nil
	}
	if n > len(s.entries) {
		n = len(s.entries)
	}
	out := make([]Entry, n)
	copy(out, s.entries[len(s.entries)-n:])
	return out
}

// Subscribe returns a channel that receives new log entries and an unsubscribe
// function. The caller must call unsubscribe when done to avoid leaking the channel.
func (s *Store) Subscribe() (<-chan Entry, func()) {
	ch := make(chan Entry, 64)
	s.mu.Lock()
	s.subs[ch] = struct{}{}
	s.mu.Unlock()
	return ch, func() {
		s.mu.Lock()
		if _, ok := s.subs[ch]; ok {
			delete(s.subs, ch)
			close(ch)
		}
		s.mu.Unlock()
	}
}

func (s *Store) append(e Entry) {
	s.mu.Lock()
	if len(s.entries) >= maxEntries {
		s.entries = s.entries[1:]
	}
	s.entries = append(s.entries, e)
	subs := make([]chan Entry, 0, len(s.subs))
	for ch := range s.subs {
		subs = append(subs, ch)
	}
	s.mu.Unlock()
	for _, ch := range subs {
		select {
		case ch <- e:
		default: // drop if subscriber is slow
		}
	}
}

// teeHandler implements slog.Handler: it delegates every record to wrapped and
// also records a simplified Entry in the store.
type teeHandler struct {
	store    *Store
	wrapped  slog.Handler
	preAttrs []slog.Attr
	group    string
}

func (h *teeHandler) Enabled(ctx context.Context, lvl slog.Level) bool {
	return h.wrapped.Enabled(ctx, lvl)
}

func (h *teeHandler) Handle(ctx context.Context, r slog.Record) error {
	werr := h.wrapped.Handle(ctx, r)

	entry := Entry{
		Time:    r.Time,
		Level:   r.Level.String(),
		Message: r.Message,
	}

	// Collect pre-attached attrs (from WithAttrs) plus record-level attrs.
	if len(h.preAttrs) > 0 || r.NumAttrs() > 0 {
		attrs := make(map[string]string)
		for _, a := range h.preAttrs {
			attrs[a.Key] = a.Value.String()
		}
		r.Attrs(func(a slog.Attr) bool {
			attrs[a.Key] = a.Value.String()
			return true
		})
		entry.Attrs = attrs
	}

	h.store.append(entry)
	return werr
}

func (h *teeHandler) WithAttrs(attrs []slog.Attr) slog.Handler {
	merged := make([]slog.Attr, len(h.preAttrs)+len(attrs))
	copy(merged, h.preAttrs)
	copy(merged[len(h.preAttrs):], attrs)
	return &teeHandler{
		store:    h.store,
		wrapped:  h.wrapped.WithAttrs(attrs),
		preAttrs: merged,
		group:    h.group,
	}
}

func (h *teeHandler) WithGroup(name string) slog.Handler {
	return &teeHandler{
		store:    h.store,
		wrapped:  h.wrapped.WithGroup(name),
		preAttrs: h.preAttrs,
		group:    name,
	}
}
