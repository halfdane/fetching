package spotify

import (
	"encoding/json"
	"testing"
)

// ---- AudioFile.Extension ----

func TestAudioFileExtension(t *testing.T) {
	cases := []struct {
		format string
		want   string
	}{
		{"OGG_VORBIS_320", ".ogg"},
		{"OGG_VORBIS_160", ".ogg"},
		{"OGG_VORBIS_96", ".ogg"},
		{"MP3_320", ".mp3"},
		{"MP3_160", ".mp3"},
		{"AAC_320", ".m4a"},
		{"AAC_160", ".m4a"},
		{"XHE_AAC_24", ".m4a"},
		{"MP4_128", ".m4a"},
		{"FLAC_FLAC", ".flac"},
		{"FLAC_FLAC_24BIT", ".flac"},
		{"UNKNOWN_FORMAT", ".ogg"}, // default fallback
	}
	for _, c := range cases {
		af := AudioFile{Format: c.format}
		if got := af.Extension(); got != c.want {
			t.Errorf("Extension(%q) = %q, want %q", c.format, got, c.want)
		}
	}
}

// ---- PreferAudioFile ----

func TestPreferAudioFileEmpty(t *testing.T) {
	if got := PreferAudioFile(nil); got != nil {
		t.Errorf("expected nil for empty list, got %+v", got)
	}
	if got := PreferAudioFile([]AudioFile{}); got != nil {
		t.Errorf("expected nil for empty list, got %+v", got)
	}
}

func TestPreferAudioFileSelectsBest(t *testing.T) {
	files := []AudioFile{
		{FileID: "a", Format: "MP3_320"},
		{FileID: "b", Format: "FLAC_FLAC"},
		{FileID: "c", Format: "OGG_VORBIS_96"},
	}
	got := PreferAudioFile(files)
	if got.FileID != "b" {
		t.Errorf("expected FLAC_FLAC (id=b), got format=%s id=%s", got.Format, got.FileID)
	}
}

func TestPreferAudioFileSingle(t *testing.T) {
	files := []AudioFile{{FileID: "x", Format: "OGG_VORBIS_320"}}
	got := PreferAudioFile(files)
	if got.FileID != "x" {
		t.Errorf("expected id=x, got %s", got.FileID)
	}
}

func TestPreferAudioFileUnknownFormatsLast(t *testing.T) {
	files := []AudioFile{
		{FileID: "unknown", Format: "WEIRD_FORMAT"},
		{FileID: "known", Format: "OGG_VORBIS_160"},
	}
	got := PreferAudioFile(files)
	if got.FileID != "known" {
		t.Errorf("expected known format to win, got %s / %s", got.FileID, got.Format)
	}
}

// ---- ParseMetadata ----

func TestParseMetadataTrack(t *testing.T) {
	raw := `{"type":"track","uri":"spotify:track:abc","name":"Test Track","album":{"uri":"spotify:album:x","name":"Test Album"},"number":3}`
	meta, err := ParseMetadata([]byte(raw))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	track, ok := meta.(*Track)
	if !ok {
		t.Fatalf("expected *Track, got %T", meta)
	}
	if track.Name != "Test Track" {
		t.Errorf("name = %q, want %q", track.Name, "Test Track")
	}
	if track.Number != 3 {
		t.Errorf("number = %d, want 3", track.Number)
	}
}

func TestParseMetadataAlbum(t *testing.T) {
	raw := `{"type":"album","uri":"spotify:album:x","name":"Album Name","discs":[{"number":1,"tracks":["spotify:track:1","spotify:track:2"]}]}`
	meta, err := ParseMetadata([]byte(raw))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	album, ok := meta.(*Album)
	if !ok {
		t.Fatalf("expected *Album, got %T", meta)
	}
	if album.Name != "Album Name" {
		t.Errorf("name = %q", album.Name)
	}
	if len(album.Discs) != 1 || len(album.Discs[0].Tracks) != 2 {
		t.Errorf("unexpected discs/tracks: %+v", album.Discs)
	}
}

func TestParseMetadataPlaylist(t *testing.T) {
	raw := `{"type":"playlist","uri":"spotify:playlist:p","name":"My Mix","track_uris":["spotify:track:1"]}`
	meta, err := ParseMetadata([]byte(raw))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	pl, ok := meta.(*Playlist)
	if !ok {
		t.Fatalf("expected *Playlist, got %T", meta)
	}
	if pl.Name != "My Mix" {
		t.Errorf("name = %q", pl.Name)
	}
}

func TestParseMetadataUnknownType(t *testing.T) {
	raw := `{"type":"invalid_type"}`
	_, err := ParseMetadata([]byte(raw))
	if err == nil {
		t.Error("expected error for unknown type, got nil")
	}
}

func TestParseMetadataInvalidJSON(t *testing.T) {
	_, err := ParseMetadata([]byte(`{broken`))
	if err == nil {
		t.Error("expected error for invalid JSON, got nil")
	}
}

// ---- TrackURIs ----

func TestTrackURIsFromTrack(t *testing.T) {
	track := &Track{URI: "spotify:track:abc"}
	uris := TrackURIs(track)
	if len(uris) != 1 || uris[0] != "spotify:track:abc" {
		t.Errorf("unexpected uris: %v", uris)
	}
}

func TestTrackURIsFromAlbum(t *testing.T) {
	album := &Album{
		Discs: []Disc{
			{Tracks: []string{"spotify:track:1", "spotify:track:2"}},
			{Tracks: []string{"spotify:track:3"}},
		},
	}
	uris := TrackURIs(album)
	if len(uris) != 3 {
		t.Errorf("expected 3 uris, got %d: %v", len(uris), uris)
	}
}

func TestTrackURIsFromPlaylist(t *testing.T) {
	pl := &Playlist{TrackURIs: []string{"a", "b"}}
	uris := TrackURIs(pl)
	if len(uris) != 2 {
		t.Errorf("expected 2 uris, got %v", uris)
	}
}

func TestTrackURIsFromShow(t *testing.T) {
	show := &Show{EpisodeURIs: []string{"e1", "e2", "e3"}}
	uris := TrackURIs(show)
	if len(uris) != 3 {
		t.Errorf("expected 3 uris, got %v", uris)
	}
}

func TestTrackURIsUnknownType(t *testing.T) {
	uris := TrackURIs("not a spotify type")
	if len(uris) != 0 {
		t.Errorf("expected nil/empty slice, got %v", uris)
	}
}

// ---- Cover helpers ----

func TestLargeCoverPreferLarge(t *testing.T) {
	covers := []Cover{
		{FileID: "small", Size: "DEFAULT"},
		{FileID: "big", Size: "LARGE"},
	}
	c := LargeCover(covers)
	if c.FileID != "big" {
		t.Errorf("expected LARGE cover, got %s", c.FileID)
	}
}

func TestLargeCoverFallsBackToDefault(t *testing.T) {
	covers := []Cover{
		{FileID: "first", Size: "DEFAULT"},
	}
	c := LargeCover(covers)
	if c.FileID != "first" {
		t.Errorf("expected DEFAULT fallback, got %s", c.FileID)
	}
}

func TestLargeCoverNil(t *testing.T) {
	if c := LargeCover(nil); c != nil {
		t.Errorf("expected nil for empty covers, got %+v", c)
	}
}

func TestDefaultCoverFallsBackToFirst(t *testing.T) {
	covers := []Cover{{FileID: "only", Size: "LARGE"}}
	c := DefaultCover(covers)
	if c.FileID != "only" {
		t.Errorf("expected first cover as fallback, got %s", c.FileID)
	}
}

// ---- ISRC / UPC ----

func TestISRC(t *testing.T) {
	ids := []ExternalID{
		{Type: "upc", ID: "00000000"},
		{Type: "isrc", ID: "GBUM71029604"},
	}
	if got := ISRC(ids); got != "GBUM71029604" {
		t.Errorf("ISRC = %q", got)
	}
}

func TestISRCMissing(t *testing.T) {
	if got := ISRC([]ExternalID{{Type: "upc", ID: "x"}}); got != "" {
		t.Errorf("expected empty ISRC, got %q", got)
	}
}

func TestUPC(t *testing.T) {
	ids := []ExternalID{{Type: "upc", ID: "602577904004"}}
	if got := UPC(ids); got != "602577904004" {
		t.Errorf("UPC = %q", got)
	}
}

// ---- CoverURL ----

func TestCoverURL(t *testing.T) {
	url := CoverURL("ab1234")
	want := "https://i.scdn.co/image/ab1234"
	if url != want {
		t.Errorf("CoverURL = %q, want %q", url, want)
	}
}

// ---- Round-trip JSON ----

func TestTrackAudioFilesJSONField(t *testing.T) {
	// The Spotify API uses "files" not "audio_files" for tracks — regression test.
	raw := `{"type":"track","uri":"u","name":"n","files":[{"file_id":"f1","format":"OGG_VORBIS_320"}]}`
	meta, err := ParseMetadata([]byte(raw))
	if err != nil {
		t.Fatal(err)
	}
	track := meta.(*Track)
	if len(track.AudioFiles) != 1 || track.AudioFiles[0].FileID != "f1" {
		t.Errorf("expected audio file f1, got %+v", track.AudioFiles)
	}
}

func TestEpisodeAudioFilesJSONField(t *testing.T) {
	// Episodes use "audio_files".
	raw := `{"type":"episode","uri":"u","name":"n","audio_files":[{"file_id":"e1","format":"OGG_VORBIS_96"}]}`
	meta, err := ParseMetadata([]byte(raw))
	if err != nil {
		t.Fatal(err)
	}
	ep := meta.(*Episode)
	if len(ep.AudioFiles) != 1 || ep.AudioFiles[0].FileID != "e1" {
		t.Errorf("expected audio file e1, got %+v", ep.AudioFiles)
	}
}

// ensure json package is used (avoids import cycle lint)
var _ = json.Marshal
