package progress

import (
	"sort"
	"sync"
)

// TrackStatus is the current state of one track in a collection job.
type TrackStatus string

const (
	TrackQueued            TrackStatus = "queued"
	TrackResolvingMetadata TrackStatus = "resolving_metadata"
	TrackFetchingAudio     TrackStatus = "fetching_audio"
	TrackRetryWaiting      TrackStatus = "retry_waiting"
	TrackAlreadyPresent    TrackStatus = "already_present"
	TrackDone              TrackStatus = "done"
	TrackFailed            TrackStatus = "failed"
)

// TrackView is the frontend-facing current state for one track.
type TrackView struct {
	TrackURI     string      `json:"trackUri"`
	Title        string      `json:"title"`
	DurationSec  int         `json:"durationSec"`
	Status       TrackStatus `json:"status"`
	RetryAttempt int         `json:"retryAttempt"`
	RetryMax     int         `json:"retryMax"`
	ErrorMessage string      `json:"errorMessage,omitempty"`
}

// CollectionView is the frontend-facing state for one submitted URI/job.
type CollectionView struct {
	JobID            int64       `json:"jobId"`
	SourceURI        string      `json:"sourceUri"`
	Kind             string      `json:"kind"`
	Title            string      `json:"title"`
	CoverURL         string      `json:"coverUrl,omitempty"`
	PlaceholderCover bool        `json:"placeholderCover"`
	TotalTracks      int         `json:"totalTracks"`
	DoneTracks       int         `json:"doneTracks"`
	FailedTracks     int         `json:"failedTracks"`
	InProgressTracks int         `json:"inProgressTracks"`
	Terminal         bool        `json:"terminal"`
	Tracks           []TrackView `json:"tracks"`
}

// Event is sent over SSE whenever the snapshot changes.
type Event struct {
	Type        string           `json:"type"`
	Collections []CollectionView `json:"collections"`
}

// Store keeps in-memory progress snapshots and broadcasts updates for SSE.
type Store struct {
	mu    sync.RWMutex
	byID  map[int64]*CollectionView
	order []int64
	subs  map[chan Event]struct{}
}

func NewStore() *Store {
	return &Store{
		byID: make(map[int64]*CollectionView),
		subs: make(map[chan Event]struct{}),
	}
}

// JobSummary is the minimal information needed to seed the progress store
// from persisted queue data on startup. Kept here to avoid importing the
// queue package.
type JobSummary struct {
	ID        int64
	SourceURI string
	Terminal  bool // true when status is "done" or "failed"
}

// SeedFromJobs populates the store with stub entries for jobs that are not
// yet complete. This ensures the worker re-displays in-progress or pending
// jobs in the web UI after a server restart. Terminal (done/failed) jobs are
// intentionally skipped — their full metadata (title, tracks, cover) was
// never persisted, so showing a bare URI stub would be misleading.
// Jobs are inserted oldest-first so the order slice matches the original
// insertion order.
func (s *Store) SeedFromJobs(jobs []JobSummary) {
	s.mu.Lock()
	for _, j := range jobs {
		if j.Terminal {
			continue // can't reconstruct metadata for completed jobs
		}
		if _, exists := s.byID[j.ID]; exists {
			continue
		}
		c := &CollectionView{
			JobID:            j.ID,
			SourceURI:        j.SourceURI,
			Kind:             "collection",
			Title:            j.SourceURI,
			PlaceholderCover: true,
			Terminal:         false,
			Tracks:           []TrackView{},
		}
		s.byID[j.ID] = c
		s.order = append(s.order, j.ID)
	}
	s.mu.Unlock()
	// Don't broadcast — this is called before any subscribers exist.
}

func (s *Store) UpsertSubmitted(jobID int64, sourceURI string) {
	s.mu.Lock()
	if _, ok := s.byID[jobID]; !ok {
		s.byID[jobID] = &CollectionView{
			JobID:            jobID,
			SourceURI:        sourceURI,
			Kind:             "collection",
			Title:            sourceURI,
			PlaceholderCover: true,
			Tracks:           []TrackView{},
		}
		s.order = append([]int64{jobID}, s.order...)
	}
	s.mu.Unlock()
	s.broadcastSnapshot()
}

func (s *Store) SetCollectionMeta(jobID int64, kind, title, coverURL string, totalTracks int) {
	s.mu.Lock()
	c := s.ensure(jobID)
	c.Kind = kind
	if title != "" {
		c.Title = title
	}
	c.CoverURL = coverURL
	c.PlaceholderCover = coverURL == ""
	if totalTracks >= 0 {
		c.TotalTracks = totalTracks
	}
	s.recompute(c)
	s.mu.Unlock()
	s.broadcastSnapshot()
}

func (s *Store) SetTrackQueued(jobID int64, trackURI string) {
	s.mu.Lock()
	c := s.ensure(jobID)
	idx := s.findTrack(c, trackURI)
	if idx == -1 {
		c.Tracks = append(c.Tracks, TrackView{
			TrackURI: trackURI,
			Status:   TrackQueued,
		})
	} else {
		c.Tracks[idx].Status = TrackQueued
	}
	s.recompute(c)
	s.mu.Unlock()
	s.broadcastSnapshot()
}

func (s *Store) UpdateTrack(jobID int64, trackURI string, update func(*TrackView)) {
	s.mu.Lock()
	c := s.ensure(jobID)
	idx := s.findTrack(c, trackURI)
	if idx == -1 {
		c.Tracks = append(c.Tracks, TrackView{TrackURI: trackURI, Status: TrackQueued})
		idx = len(c.Tracks) - 1
	}
	update(&c.Tracks[idx])
	s.recompute(c)
	s.mu.Unlock()
	s.broadcastSnapshot()
}

func (s *Store) MarkCollectionTerminal(jobID int64) {
	s.mu.Lock()
	c := s.ensure(jobID)
	c.Terminal = true
	s.recompute(c)
	s.mu.Unlock()
	s.broadcastSnapshot()
}

// SnapshotJob returns a copy of the CollectionView for the given job, or nil
// if it is not present in the store.
func (s *Store) SnapshotJob(id int64) *CollectionView {
	s.mu.Lock()
	defer s.mu.Unlock()
	c, ok := s.byID[id]
	if !ok {
		return nil
	}
	cp := *c
	if len(c.Tracks) > 0 {
		cp.Tracks = append([]TrackView(nil), c.Tracks...)
	} else {
		cp.Tracks = []TrackView{}
	}
	return &cp
}

// SeedFromResults populates the store with fully-formed CollectionViews
// recovered from persisted job results. Called on startup before any
// subscribers exist, so no broadcast is needed.
func (s *Store) SeedFromResults(views []CollectionView) {
	s.mu.Lock()
	for _, v := range views {
		if _, exists := s.byID[v.JobID]; exists {
			continue
		}
		cp := v
		if len(v.Tracks) > 0 {
			cp.Tracks = append([]TrackView(nil), v.Tracks...)
		} else {
			cp.Tracks = []TrackView{}
		}
		s.byID[v.JobID] = &cp
		s.order = append(s.order, v.JobID)
	}
	s.mu.Unlock()
}

// Remove deletes one or more collections from the store and broadcasts
// the updated snapshot. Used when retrying — old terminal entries are
// removed before the fresh job is inserted.
func (s *Store) Remove(jobIDs ...int64) {
	if len(jobIDs) == 0 {
		return
	}
	s.mu.Lock()
	for _, id := range jobIDs {
		delete(s.byID, id)
	}
	keep := s.order[:0]
	toRemove := make(map[int64]bool, len(jobIDs))
	for _, id := range jobIDs {
		toRemove[id] = true
	}
	for _, id := range s.order {
		if !toRemove[id] {
			keep = append(keep, id)
		}
	}
	s.order = keep
	s.mu.Unlock()
	s.broadcastSnapshot()
}

func (s *Store) Snapshot() []CollectionView {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return s.snapshotLocked()
}

func (s *Store) Subscribe() (<-chan Event, func()) {
	ch := make(chan Event, 8)
	s.mu.Lock()
	s.subs[ch] = struct{}{}
	initial := Event{Type: "snapshot", Collections: s.snapshotLocked()}
	s.mu.Unlock()
	ch <- initial
	unsub := func() {
		s.mu.Lock()
		if _, ok := s.subs[ch]; ok {
			delete(s.subs, ch)
			close(ch)
		}
		s.mu.Unlock()
	}
	return ch, unsub
}

func (s *Store) broadcastSnapshot() {
	s.mu.RLock()
	e := Event{Type: "snapshot", Collections: s.snapshotLocked()}
	subs := make([]chan Event, 0, len(s.subs))
	for ch := range s.subs {
		subs = append(subs, ch)
	}
	s.mu.RUnlock()
	for _, ch := range subs {
		select {
		case ch <- e:
		default:
		}
	}
}

func (s *Store) ensure(jobID int64) *CollectionView {
	if c, ok := s.byID[jobID]; ok {
		return c
	}
	c := &CollectionView{
		JobID:            jobID,
		SourceURI:        "",
		Kind:             "collection",
		Title:            "",
		PlaceholderCover: true,
		Tracks:           []TrackView{},
	}
	s.byID[jobID] = c
	s.order = append([]int64{jobID}, s.order...)
	return c
}

func (s *Store) findTrack(c *CollectionView, trackURI string) int {
	for i := range c.Tracks {
		if c.Tracks[i].TrackURI == trackURI {
			return i
		}
	}
	return -1
}

func (s *Store) recompute(c *CollectionView) {
	done := 0
	failed := 0
	inProgress := 0
	for _, t := range c.Tracks {
		switch t.Status {
		case TrackDone, TrackAlreadyPresent:
			done++
		case TrackFailed:
			failed++
		default:
			inProgress++
		}
	}
	c.DoneTracks = done
	c.FailedTracks = failed
	c.InProgressTracks = inProgress
	if c.TotalTracks == 0 {
		c.TotalTracks = len(c.Tracks)
	}
}

func (s *Store) snapshotLocked() []CollectionView {
	if len(s.order) == 0 {
		return nil
	}
	seen := make(map[int64]bool)
	out := make([]CollectionView, 0, len(s.order))
	for _, id := range s.order {
		if seen[id] {
			continue
		}
		seen[id] = true
		if c, ok := s.byID[id]; ok {
			cp := *c
			cp.Tracks = append([]TrackView(nil), c.Tracks...)
			out = append(out, cp)
		}
	}
	sort.SliceStable(out, func(i, j int) bool { return out[i].JobID > out[j].JobID })
	return out
}
