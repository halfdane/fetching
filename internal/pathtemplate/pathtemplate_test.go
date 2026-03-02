package pathtemplate

import (
	"strings"
	"testing"
)

func TestExpandTrackDefault(t *testing.T) {
	got, err := ExpandTrack(DefaultTrack, TrackTokens{
		Artist: "Pink Floyd", Album: "The Wall", Title: "Comfortably Numb", TrackNumber: 6,
	})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	want := "Pink Floyd/The Wall/06-Comfortably Numb"
	if got != want {
		t.Errorf("got %q, want %q", got, want)
	}
}

func TestExpandTrackYearPrefix(t *testing.T) {
	got, err := ExpandTrack("{artist}/{year}-{album}/{track_number}-{title}", TrackTokens{
		Artist: "Pink Floyd", Album: "The Dark Side of the Moon",
		Title: "Money", TrackNumber: 6, Year: "1973",
	})
	if err != nil {
		t.Fatal(err)
	}
	want := "Pink Floyd/1973-The Dark Side of the Moon/06-Money"
	if got != want {
		t.Errorf("got %q, want %q", got, want)
	}
}

func TestExpandTrackFlatNoDir(t *testing.T) {
	got, err := ExpandTrack("{track_number}-{artist}-{title}", TrackTokens{
		Artist: "Artist", Title: "Song", TrackNumber: 1,
	})
	if err != nil {
		t.Fatal(err)
	}
	if got != "01-Artist-Song" {
		t.Errorf("got %q", got)
	}
}

func TestExpandTrackDiscNumberOmittedWhenOne(t *testing.T) {
	// disc_number produces empty string when DiscNumber <= 1
	got, err := ExpandTrack("{disc_number}/{track_number}-{title}", TrackTokens{
		Title: "Song", TrackNumber: 3, DiscNumber: 1,
	})
	if err != nil {
		t.Fatal(err)
	}
	// empty disc segment gets trimmed to "_/03-Song"
	if !strings.HasSuffix(got, "/03-Song") {
		t.Errorf("got %q, expected to end with /03-Song", got)
	}
}

func TestExpandTrackDiscNumberWhenMulti(t *testing.T) {
	got, err := ExpandTrack("{disc_number}-{track_number}-{title}", TrackTokens{
		Title: "Song", TrackNumber: 3, DiscNumber: 2,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(got, "2-03-Song") {
		t.Errorf("got %q, expected to start with 2-03-Song", got)
	}
}

func TestExpandTrackAlbumArtist(t *testing.T) {
	got, err := ExpandTrack("{album_artist}/{album}/{track_number}-{title}", TrackTokens{
		Artist: "Performer", AlbumArtist: "Composer",
		Album: "Album", Title: "Song", TrackNumber: 1,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(got, "Composer/") {
		t.Errorf("expected album_artist=Composer, got %q", got)
	}
}

func TestExpandTrackSanitizesArtist(t *testing.T) {
	got, err := ExpandTrack("{artist}/{album}/{track_number}-{title}", TrackTokens{
		Artist: "AC/DC", Album: "Back in Black", Title: "Hells Bells", TrackNumber: 1,
	})
	if err != nil {
		t.Fatal(err)
	}
	// "/" in artist should not create an extra path segment
	if strings.HasPrefix(got, "AC/DC/") {
		t.Errorf("artist slash leaked into path: %q", got)
	}
	if !strings.HasPrefix(got, "AC_DC/") {
		t.Errorf("expected AC_DC, got %q", got)
	}
}

func TestExpandTrackUnknownToken(t *testing.T) {
	_, err := ExpandTrack("{artist}/{unknown_token}/{title}", TrackTokens{})
	if err == nil {
		t.Error("expected error for unknown token, got nil")
	}
}

func TestExpandTrackUnclosedBrace(t *testing.T) {
	_, err := ExpandTrack("{artist/{album}", TrackTokens{})
	if err == nil {
		t.Error("expected error for unclosed brace, got nil")
	}
}

func TestExpandEpisodeDefault(t *testing.T) {
	got, err := ExpandEpisode(DefaultEpisode, EpisodeTokens{
		Show: "My Podcast", Title: "Episode 1",
	})
	if err != nil {
		t.Fatal(err)
	}
	if got != "My Podcast/Episode 1" {
		t.Errorf("got %q", got)
	}
}

func TestExpandEpisodeWithYear(t *testing.T) {
	got, err := ExpandEpisode("{show}/{year}/{title}", EpisodeTokens{
		Show: "The Show", Title: "Ep 42", Year: "2024",
	})
	if err != nil {
		t.Fatal(err)
	}
	if got != "The Show/2024/Ep 42" {
		t.Errorf("got %q", got)
	}
}

func TestExpandEpisodeNumber(t *testing.T) {
	got, err := ExpandEpisode("{show}/{episode_number}-{title}", EpisodeTokens{
		Show: "Show", Title: "Title", EpisodeNumber: 7,
	})
	if err != nil {
		t.Fatal(err)
	}
	if got != "Show/07-Title" {
		t.Errorf("got %q", got)
	}
}

func TestValidateTrackGood(t *testing.T) {
	if err := ValidateTrack(DefaultTrack); err != nil {
		t.Errorf("default template should validate: %v", err)
	}
}

func TestValidateTrackBad(t *testing.T) {
	if err := ValidateTrack("{artist}/{NOPE}/{title}"); err == nil {
		t.Error("expected error for bad token")
	}
}

func TestValidateEpisodeGood(t *testing.T) {
	if err := ValidateEpisode(DefaultEpisode); err != nil {
		t.Errorf("default template should validate: %v", err)
	}
}
