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

// ---- TrackPath — default template ----

func TestTrackPathDefault(t *testing.T) {
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

func TestPlaylistDir(t *testing.T) {
	s := New("/media")
	got := s.PlaylistDir("Chill Vibes")
	want := filepath.Join("/media", "Playlists", "Chill Vibes")
	if got != want {
		t.Errorf("PlaylistDir = %q, want %q", got, want)
	}
}

// ---- TrackPath — custom templates ----

func TestTrackPathCustomTemplate(t *testing.T) {
	s := NewWithTemplates("/music", "{artist}/{year}-{album}/{track_number}-{title}", "")
	track := &spotify.Track{
		Name:    "Brain Damage",
		Number:  9,
		Album:   spotify.AlbumRef{Name: "The Dark Side of the Moon", Date: "1973-03-01"},
		Artists: []spotify.Artist{{Name: "Pink Floyd"}},
	}
	got := s.TrackPath(track, ".ogg")
	want := filepath.Join("/music", "Pink Floyd", "1973-The Dark Side of the Moon", "09-Brain Damage.ogg")
	if got != want {
		t.Errorf("custom template: %q, want %q", got, want)
	}
}

func TestTrackPathFlatTemplate(t *testing.T) {
	s := NewWithTemplates("/music", "{track_number}-{artist}-{title}", "")
	track := &spotify.Track{
		Name: "Song", Number: 1,
		Album:   spotify.AlbumRef{Name: "Album"},
		Artists: []spotify.Artist{{Name: "Artist"}},
	}
	got := s.TrackPath(track, ".mp3")
	want := filepath.Join("/music", "01-Artist-Song.mp3")
	if got != want {
		t.Errorf("flat template: %q, want %q", got, want)
	}
}

func TestNewWithTemplatesEmptyUsesDefaults(t *testing.T) {
	track := &spotify.Track{
		Name: "Song", Number: 1,
		Album:   spotify.AlbumRef{Name: "Album"},
		Artists: []spotify.Artist{{Name: "Artist"}},
	}
	if New("/music").TrackPath(track, ".ogg") != NewWithTemplates("/music", "", "").TrackPath(track, ".ogg") {
		t.Error("NewWithTemplates(\"\") should equal New()")
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

// ---- Path containment / traversal guard ----

func TestWithinBaseDirAllowsNormal(t *testing.T) {
	s := New("/music")
	if err := s.withinBaseDir("/music/artist/track.flac"); err != nil {
		t.Errorf("unexpected error for normal path: %v", err)
	}
}

func TestWithinBaseDirAllowsBaseItself(t *testing.T) {
	s := New("/music")
	if err := s.withinBaseDir("/music"); err != nil {
		t.Errorf("unexpected error for base dir itself: %v", err)
	}
}

func TestWithinBaseDirNormalisesDotsInsideBase(t *testing.T) {
	s := New("/music")
	// /music/../music/track.flac cleans to /music/track.flac — still inside base
	if err := s.withinBaseDir("/music/../music/track.flac"); err != nil {
		t.Errorf("unexpected error for normalised path: %v", err)
	}
}

func TestWithinBaseDirBlocksTraversal(t *testing.T) {
	s := New("/music")
	// Would resolve to /etc/shadow
	if err := s.withinBaseDir("/music/../../etc/shadow"); err == nil {
		t.Error("expected error for traversal path, got nil")
	}
}

func TestWithinBaseDirBlocksSibling(t *testing.T) {
	s := New("/music")
	// /musicother is not inside /music (prefix trick: "startswith /music" is insufficient)
	if err := s.withinBaseDir("/musicother/track.flac"); err == nil {
		t.Error("expected error for sibling directory with same prefix, got nil")
	}
}

func TestCreateTrackWriterBlocksTraversalTemplate(t *testing.T) {
	// A template containing ".." literal segments should be rejected when the
	// resulting path would escape the base directory.
	s := NewWithTemplates("/music", "../../{title}", "")
	track := &spotify.Track{
		Name:  "shadow",
		Album: spotify.AlbumRef{Name: "A", Artists: []spotify.Artist{{Name: "B"}}},
		URI:   "spotify:track:1",
	}
	_, _, err := s.CreateTrackWriter(track, ".flac")
	if err == nil {
		t.Error("expected containment error for traversal template, got nil")
	}
}
