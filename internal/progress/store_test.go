package progress

import (
	"testing"
	"time"
)

func TestNewStoreSnapshotEmpty(t *testing.T) {
	s := NewStore()
	snap := s.Snapshot()
	if snap != nil {
		t.Fatalf("expected nil snapshot for empty store, got %d items", len(snap))
	}
}

func TestUpsertSubmittedCreatesCollection(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "spotify:album:abc")

	snap := s.Snapshot()
	if len(snap) != 1 {
		t.Fatalf("expected 1 collection, got %d", len(snap))
	}
	c := snap[0]
	if c.JobID != 1 {
		t.Errorf("JobID = %d, want 1", c.JobID)
	}
	if c.SourceURI != "spotify:album:abc" {
		t.Errorf("SourceURI = %q, want %q", c.SourceURI, "spotify:album:abc")
	}
	if c.Kind != "collection" {
		t.Errorf("Kind = %q, want %q", c.Kind, "collection")
	}
	if !c.PlaceholderCover {
		t.Error("PlaceholderCover should be true initially")
	}
}

func TestUpsertSubmittedIsIdempotent(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "spotify:album:abc")
	s.UpsertSubmitted(1, "spotify:album:abc")

	snap := s.Snapshot()
	if len(snap) != 1 {
		t.Fatalf("expected 1 collection after double upsert, got %d", len(snap))
	}
}

func TestSetCollectionMeta(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "spotify:album:abc")
	s.SetCollectionMeta(1, "album", "My Album", "https://cover/abc", 12)

	snap := s.Snapshot()
	c := snap[0]
	if c.Kind != "album" {
		t.Errorf("Kind = %q, want %q", c.Kind, "album")
	}
	if c.Title != "My Album" {
		t.Errorf("Title = %q, want %q", c.Title, "My Album")
	}
	if c.CoverURL != "https://cover/abc" {
		t.Errorf("CoverURL = %q, want %q", c.CoverURL, "https://cover/abc")
	}
	if c.PlaceholderCover {
		t.Error("PlaceholderCover should be false when cover is set")
	}
	if c.TotalTracks != 12 {
		t.Errorf("TotalTracks = %d, want 12", c.TotalTracks)
	}
}

func TestSetCollectionMetaPlaceholderCoverWhenEmpty(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "spotify:playlist:xyz")
	s.SetCollectionMeta(1, "playlist", "My Playlist", "", 5)

	c := s.Snapshot()[0]
	if !c.PlaceholderCover {
		t.Error("PlaceholderCover should be true when coverURL is empty")
	}
}

func TestSetTrackQueued(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "spotify:album:abc")
	s.SetTrackQueued(1, "spotify:track:t1")
	s.SetTrackQueued(1, "spotify:track:t2")

	c := s.Snapshot()[0]
	if len(c.Tracks) != 2 {
		t.Fatalf("expected 2 tracks, got %d", len(c.Tracks))
	}
	for _, tr := range c.Tracks {
		if tr.Status != TrackQueued {
			t.Errorf("track %s status = %q, want %q", tr.TrackURI, tr.Status, TrackQueued)
		}
	}
}

func TestSetTrackQueuedIdempotent(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "spotify:album:abc")
	s.SetTrackQueued(1, "spotify:track:t1")
	s.SetTrackQueued(1, "spotify:track:t1")

	c := s.Snapshot()[0]
	if len(c.Tracks) != 1 {
		t.Fatalf("expected 1 track after double queue, got %d", len(c.Tracks))
	}
}

func TestUpdateTrackStatus(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "spotify:album:abc")
	s.SetTrackQueued(1, "spotify:track:t1")

	s.UpdateTrack(1, "spotify:track:t1", func(tv *TrackView) {
		tv.Status = TrackFetchingAudio
		tv.Title = "My Track"
		tv.DurationSec = 210
	})

	c := s.Snapshot()[0]
	tr := c.Tracks[0]
	if tr.Status != TrackFetchingAudio {
		t.Errorf("Status = %q, want %q", tr.Status, TrackFetchingAudio)
	}
	if tr.Title != "My Track" {
		t.Errorf("Title = %q, want %q", tr.Title, "My Track")
	}
	if tr.DurationSec != 210 {
		t.Errorf("DurationSec = %d, want 210", tr.DurationSec)
	}
}

func TestUpdateTrackCreatesIfMissing(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "spotify:album:abc")

	s.UpdateTrack(1, "spotify:track:t1", func(tv *TrackView) {
		tv.Status = TrackFetchingAudio
	})

	c := s.Snapshot()[0]
	if len(c.Tracks) != 1 {
		t.Fatalf("expected 1 track, got %d", len(c.Tracks))
	}
	if c.Tracks[0].Status != TrackFetchingAudio {
		t.Errorf("Status = %q, want %q", c.Tracks[0].Status, TrackFetchingAudio)
	}
}

func TestRecomputeCounters(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "spotify:album:abc")
	s.SetCollectionMeta(1, "album", "A", "", 3)
	s.SetTrackQueued(1, "spotify:track:t1")
	s.SetTrackQueued(1, "spotify:track:t2")
	s.SetTrackQueued(1, "spotify:track:t3")

	s.UpdateTrack(1, "spotify:track:t1", func(tv *TrackView) {
		tv.Status = TrackDone
	})
	s.UpdateTrack(1, "spotify:track:t2", func(tv *TrackView) {
		tv.Status = TrackFailed
		tv.ErrorMessage = "something went wrong"
	})

	c := s.Snapshot()[0]
	if c.DoneTracks != 1 {
		t.Errorf("DoneTracks = %d, want 1", c.DoneTracks)
	}
	if c.FailedTracks != 1 {
		t.Errorf("FailedTracks = %d, want 1", c.FailedTracks)
	}
	if c.InProgressTracks != 1 {
		t.Errorf("InProgressTracks = %d, want 1", c.InProgressTracks)
	}
	if c.TotalTracks != 3 {
		t.Errorf("TotalTracks = %d, want 3", c.TotalTracks)
	}
}

func TestAlreadyPresentCountsAsDone(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "spotify:album:abc")
	s.SetTrackQueued(1, "spotify:track:t1")

	s.UpdateTrack(1, "spotify:track:t1", func(tv *TrackView) {
		tv.Status = TrackAlreadyPresent
	})

	c := s.Snapshot()[0]
	if c.DoneTracks != 1 {
		t.Errorf("DoneTracks = %d, want 1 (already_present should count as done)", c.DoneTracks)
	}
}

func TestMarkCollectionTerminal(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "spotify:album:abc")
	s.MarkCollectionTerminal(1)

	c := s.Snapshot()[0]
	if !c.Terminal {
		t.Error("expected collection to be terminal")
	}
}

func TestSnapshotOrderNewestFirst(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "first")
	s.UpsertSubmitted(2, "second")
	s.UpsertSubmitted(3, "third")

	snap := s.Snapshot()
	if len(snap) != 3 {
		t.Fatalf("expected 3, got %d", len(snap))
	}
	if snap[0].JobID != 3 || snap[1].JobID != 2 || snap[2].JobID != 1 {
		t.Errorf("order = [%d, %d, %d], want [3, 2, 1]", snap[0].JobID, snap[1].JobID, snap[2].JobID)
	}
}

func TestSnapshotIsDeepCopy(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "uri")
	s.SetTrackQueued(1, "spotify:track:t1")

	snap := s.Snapshot()
	snap[0].Tracks[0].Title = "MUTATED"

	snap2 := s.Snapshot()
	if snap2[0].Tracks[0].Title == "MUTATED" {
		t.Error("snapshot mutation leaked back to store — not a deep copy")
	}
}

func TestSubscribeReceivesInitialSnapshot(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "uri")

	ch, unsub := s.Subscribe()
	defer unsub()

	select {
	case ev := <-ch:
		if ev.Type != "snapshot" {
			t.Errorf("Type = %q, want %q", ev.Type, "snapshot")
		}
		if len(ev.Collections) != 1 {
			t.Fatalf("expected 1 collection in initial snapshot, got %d", len(ev.Collections))
		}
	case <-time.After(time.Second):
		t.Fatal("timed out waiting for initial snapshot")
	}
}

func TestSubscribeReceivesUpdates(t *testing.T) {
	s := NewStore()
	ch, unsub := s.Subscribe()
	defer unsub()

	// Drain initial snapshot
	<-ch

	s.UpsertSubmitted(1, "uri")

	select {
	case ev := <-ch:
		if len(ev.Collections) != 1 {
			t.Fatalf("expected 1 collection in update, got %d", len(ev.Collections))
		}
	case <-time.After(time.Second):
		t.Fatal("timed out waiting for broadcast")
	}
}

func TestUnsubscribeStopsBroadcast(t *testing.T) {
	s := NewStore()
	ch, unsub := s.Subscribe()

	// Drain initial
	<-ch

	unsub()
	s.UpsertSubmitted(1, "uri")

	select {
	case _, ok := <-ch:
		if ok {
			t.Error("expected channel to be closed after unsubscribe")
		}
	case <-time.After(100 * time.Millisecond):
		// Channel closed, no more messages — OK
	}
}

func TestRetryWaitingState(t *testing.T) {
	s := NewStore()
	s.UpsertSubmitted(1, "album")
	s.SetTrackQueued(1, "spotify:track:t1")

	s.UpdateTrack(1, "spotify:track:t1", func(tv *TrackView) {
		tv.Status = TrackRetryWaiting
		tv.RetryAttempt = 2
		tv.RetryMax = 3
		tv.ErrorMessage = "timeout"
	})

	c := s.Snapshot()[0]
	tr := c.Tracks[0]
	if tr.Status != TrackRetryWaiting {
		t.Errorf("Status = %q, want %q", tr.Status, TrackRetryWaiting)
	}
	if tr.RetryAttempt != 2 {
		t.Errorf("RetryAttempt = %d, want 2", tr.RetryAttempt)
	}
	if tr.RetryMax != 3 {
		t.Errorf("RetryMax = %d, want 3", tr.RetryMax)
	}
	if tr.ErrorMessage != "timeout" {
		t.Errorf("ErrorMessage = %q, want %q", tr.ErrorMessage, "timeout")
	}
	// RetryWaiting counts as in-progress
	if c.InProgressTracks != 1 {
		t.Errorf("InProgressTracks = %d, want 1", c.InProgressTracks)
	}
}

func TestEnsureAutoCreatesOnUpdate(t *testing.T) {
	s := NewStore()
	// Call UpdateTrack without prior UpsertSubmitted — should auto-create.
	s.UpdateTrack(42, "spotify:track:t1", func(tv *TrackView) {
		tv.Status = TrackDone
	})

	snap := s.Snapshot()
	if len(snap) != 1 {
		t.Fatalf("expected 1 collection auto-created, got %d", len(snap))
	}
	if snap[0].JobID != 42 {
		t.Errorf("JobID = %d, want 42", snap[0].JobID)
	}
}

func TestFullLifecycle(t *testing.T) {
	s := NewStore()

	// 1. Submit
	s.UpsertSubmitted(1, "spotify:album:abc")

	// 2. Set metadata
	s.SetCollectionMeta(1, "album", "Test Album", "https://cover.jpg", 2)

	// 3. Queue tracks
	s.SetTrackQueued(1, "spotify:track:t1")
	s.SetTrackQueued(1, "spotify:track:t2")

	// 4. Track 1: resolving → fetching → done
	for _, status := range []TrackStatus{TrackResolvingMetadata, TrackFetchingAudio, TrackDone} {
		s.UpdateTrack(1, "spotify:track:t1", func(tv *TrackView) {
			tv.Status = status
			if status == TrackDone {
				tv.Title = "Track One"
				tv.DurationSec = 180
			}
		})
	}

	// 5. Track 2: resolving → fail
	s.UpdateTrack(1, "spotify:track:t2", func(tv *TrackView) {
		tv.Status = TrackResolvingMetadata
	})
	s.UpdateTrack(1, "spotify:track:t2", func(tv *TrackView) {
		tv.Status = TrackFailed
		tv.ErrorMessage = "network error"
	})

	// 6. Terminal
	s.MarkCollectionTerminal(1)

	c := s.Snapshot()[0]
	if !c.Terminal {
		t.Error("expected terminal")
	}
	if c.DoneTracks != 1 {
		t.Errorf("DoneTracks = %d, want 1", c.DoneTracks)
	}
	if c.FailedTracks != 1 {
		t.Errorf("FailedTracks = %d, want 1", c.FailedTracks)
	}
	if c.InProgressTracks != 0 {
		t.Errorf("InProgressTracks = %d, want 0", c.InProgressTracks)
	}
	if c.Tracks[1].ErrorMessage != "network error" {
		t.Errorf("ErrorMessage = %q, want %q", c.Tracks[1].ErrorMessage, "network error")
	}
}
