package storage

import (
	"path/filepath"
	"testing"

	"github.com/halfdane/fetching/internal/spotify"
)

// ---- Sanitize ----

func TestSanitizeNormal(t *testing.T) {
	if got := Sanitize("Hello World"); got != "Hello World" {
		t.Errorf("Sanitize(%q) = %q", "Hello World", got)
	}
}

func TestSanitizeForbiddenChars(t *testing.T) {
	cases := []struct {
		input string
		want  string
	}{
		{`AC/DC`, "AC_DC"},
		{`Artist: Name`, "Artist_ Name"},
		{`A*B?C`, "A_B_C"},
		{`"Quoted"`, `_Quoted_`},
		{`A<B>C`, `A_B_C`},
		{`A|B`, `A_B`},
		{`A\B`, "A_B"},
	}
	for _, c := range cases {
		if got := Sanitize(c.input); got != c.want {
			t.Errorf("Sanitize(%q) = %q, want %q", c.input, got, c.want)
		}
	}
}

func TestSanitizeEmpty(t *testing.T) {
	if got := Sanitize(""); got != "_" {
		t.Errorf("Sanitize(%q) = %q, want _", "", got)
	}
}

func TestSanitizeTrimDotsAndSpaces(t *testing.T) {
	if got := Sanitize("  . leading"); got != "leading" {
		t.Errorf("got %q", got)
	}
	if got := Sanitize("trailing.  "); got != "trailing" {
		t.Errorf("got %q", got)
	}
	if got := Sanitize("... "); got != "_" {
		t.Errorf("all-dots-and-spaces should return _, got %q", got)
	}
}

// ---- Directory helpers ----

func TestAlbumDir(t *testing.T) {
	s := New("/music")
	got := s.AlbumDir("Pink Floyd", "The Wall")
	want := filepath.Join("/music", "Pink Floyd", "The Wall")
	if got != want {
		t.Errorf("AlbumDir = %q, want %q", got, want)
	}
}

func TestAlbumDirSanitizesInput(t *testing.T) {
	s := New("/music")
	got := s.AlbumDir("AC/DC", "Back in Black")
	want := filepath.Join("/music", "AC_DC", "Back in Black")
	if got != want {
		t.Errorf("AlbumDir = %q, want %q", got, want)
	}
}

func TestShowDir(t *testing.T) {
	s := New("/media")
	got := s.ShowDir("My Podcast: Season 1")
	want := filepath.Join("/media", "My Podcast_ Season 1")
	if got != want {
		t.Errorf("ShowDir = %q, want %q", got, want)
	}
}

func TestPlaylistDir(t *testing.T) {
	s := New("/media")
	got := s.PlaylistDir("Chill Vibes")
	want := filepath.Join("/media", "Playlists", "Chill Vibes")
	if got != want {
		t.Errorf("PlaylistDir = %q, want %q", got, want)
	}
}

// ---- TrackPath ----

func TestTrackPath(t *testing.T) {
	s := New("/music")
	track := &spotify.Track{
		Name:   "Comfortably Numb",
		Number: 6,
		Album:  spotify.AlbumRef{Name: "The Wall"},
		Artists: []spotify.Artist{
			{Name: "Pink Floyd"},
		},
	}
	got := s.TrackPath(track, ".ogg")
	want := filepath.Join("/music", "Pink Floyd", "The Wall", "06-Comfortably Numb.ogg")
	if got != want {
		t.Errorf("TrackPath = %q, want %q", got, want)
	}
}

func TestTrackPathNoArtist(t *testing.T) {
	s := New("/music")
	track := &spotify.Track{
		Name:   "Unknown",
		Number: 1,
		Album:  spotify.AlbumRef{Name: "Album"},
	}
	got := s.TrackPath(track, ".ogg")
	want := filepath.Join("/music", "Unknown Artist", "Album", "01-Unknown.ogg")
	if got != want {
		t.Errorf("TrackPath = %q, want %q", got, want)
	}
}

func TestTrackPathPadsNumber(t *testing.T) {
	s := New("/music")
	track := &spotify.Track{
		Name:    "Short",
		Number:  3,
		Album:   spotify.AlbumRef{Name: "Album"},
		Artists: []spotify.Artist{{Name: "Artist"}},
	}
	got := s.TrackPath(track, ".ogg")
	if filepath.Base(got) != "03-Short.ogg" {
		t.Errorf("expected zero-padded track number, got %q", filepath.Base(got))
	}
}

// ---- EpisodePath ----

func TestEpisodePath(t *testing.T) {
	s := New("/podcasts")
	ep := &spotify.Episode{
		Name:     "Episode 1",
		ShowName: "My Show",
	}
	got := s.EpisodePath(ep, ".ogg")
	want := filepath.Join("/podcasts", "My Show", "Episode 1.ogg")
	if got != want {
		t.Errorf("EpisodePath = %q, want %q", got, want)
	}
}

func TestEpisodePathFallsBackToShowURI(t *testing.T) {
	s := New("/podcasts")
	ep := &spotify.Episode{
		Name:    "Episode 1",
		ShowURI: "spotify:show:abc",
		// ShowName intentionally empty → sanitizes to "_"
	}
	got := s.EpisodePath(ep, ".ogg")
	// ShowName="" → sanitize("") = "_" → falls back to ShowURI
	want := filepath.Join("/podcasts", "spotify_show_abc", "Episode 1.ogg")
	if got != want {
		t.Errorf("EpisodePath (no show name) = %q, want %q", got, want)
	}
}
