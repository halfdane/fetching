package tagger

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/halfdane/fetching/internal/spotify"
)

// TestMain intercepts test-binary re-executions triggered by fakeExecCommand.
// When GO_WANT_HELPER_PROCESS=1 the binary acts as a minimal ffmpeg stub:
//   - Successful run: copies the first -i <input> file to the last argument
//     (the output path), simulating ffmpeg's copy-then-rename strategy.
//   - FAKE_FFMPEG_FAIL=1: exits with a non-zero status to simulate failure.
//   - FAKE_FFMPEG_ARGS_FILE=<path>: writes received cmdArgs to the file.
func TestMain(m *testing.M) {
	if os.Getenv("GO_WANT_HELPER_PROCESS") != "1" {
		os.Exit(m.Run())
	}

	// Locate the real args (everything after "--").
	args := os.Args
	for i, a := range args {
		if a == "--" {
			args = args[i+1:]
			break
		}
	}
	// args[0] is the faked binary name; args[1:] are its arguments.
	cmdArgs := args[1:]

	if os.Getenv("FAKE_FFMPEG_FAIL") == "1" {
		fmt.Fprintln(os.Stderr, "fake ffmpeg: forced failure")
		os.Exit(1)
	}

	// Record args if asked (used by arg-inspection tests).
	if argsFile := os.Getenv("FAKE_FFMPEG_ARGS_FILE"); argsFile != "" {
		_ = os.WriteFile(argsFile, []byte(strings.Join(cmdArgs, "\n")), 0644)
	}

	// Find -i <input> (first occurrence) and derive output (last arg).
	var input, output string
	for i, a := range cmdArgs {
		if a == "-i" && i+1 < len(cmdArgs) && input == "" {
			input = cmdArgs[i+1]
		}
	}
	if len(cmdArgs) > 0 {
		output = cmdArgs[len(cmdArgs)-1]
	}

	if input != "" && output != "" {
		data, _ := os.ReadFile(input)
		_ = os.WriteFile(output, data, 0644)
	}
	os.Exit(0)
}

// fakeExecCommand returns a cmdFunc that re-invokes the test binary as a fake
// ffmpeg subprocess. extraEnv entries are appended to the child environment.
func fakeExecCommand(extraEnv ...string) func(string, ...string) *exec.Cmd {
	return func(name string, args ...string) *exec.Cmd {
		cs := []string{"-test.run=TestMain", "--", name}
		cs = append(cs, args...)
		cmd := exec.Command(os.Args[0], cs...)
		cmd.Env = append(os.Environ(), "GO_WANT_HELPER_PROCESS=1")
		cmd.Env = append(cmd.Env, extraEnv...)
		return cmd
	}
}

// newTestTagger creates a Tagger wired to the fake ffmpeg subprocess.
func newTestTagger(extraEnv ...string) *Tagger {
	return &Tagger{
		Binary:  "ffmpeg",
		Verbose: false,
		cmdFunc: fakeExecCommand(extraEnv...),
	}
}

// minimalTrack returns a Track with enough fields to drive TagTrack.
func minimalTrack() *spotify.Track {
	return &spotify.Track{
		Type:       spotify.TypeTrack,
		URI:        "spotify:track:t1",
		Name:       "Test Track",
		Number:     3,
		DiscNumber: 1,
		DurationMS: 180000,
		Artists:    []spotify.Artist{{URI: "spotify:artist:a", Name: "Test Artist"}},
		Album: spotify.AlbumRef{
			URI:     "spotify:album:a",
			Name:    "Test Album",
			Date:    "1990-01-01",
			Label:   "Test Label",
			Artists: []spotify.Artist{{Name: "Test Artist"}},
		},
	}
}

// minimalEpisode returns an Episode with enough fields to drive TagEpisode.
func minimalEpisode() *spotify.Episode {
	return &spotify.Episode{
		Type:        spotify.TypeEpisode,
		URI:         "spotify:episode:e1",
		Name:        "Test Episode",
		ShowURI:     "spotify:show:s1",
		ShowName:    "Test Show",
		Description: "A test podcast episode.",
		Number:      5,
		PublishTime: "2023-06-15",
		Language:    "en",
	}
}

// makeAudioFile creates a temporary .ogg file containing dummy bytes.
func makeAudioFile(t *testing.T) string {
	t.Helper()
	f, err := os.CreateTemp(t.TempDir(), "audio-*.ogg")
	if err != nil {
		t.Fatalf("create audio temp file: %v", err)
	}
	_, _ = f.WriteString("fake ogg audio")
	f.Close()
	return f.Name()
}

// ---- TagTrack --------------------------------------------------------

// TestTagTrack_HappyPath verifies that TagTrack runs ffmpeg successfully and
// leaves the audio file in place (renamed from the .tagged temp file).
func TestTagTrack_HappyPath(t *testing.T) {
	tgr := newTestTagger()
	audioPath := makeAudioFile(t)

	if err := tgr.TagTrack(audioPath, minimalTrack()); err != nil {
		t.Fatalf("TagTrack: %v", err)
	}

	if _, err := os.Stat(audioPath); err != nil {
		t.Errorf("audio file no longer exists after TagTrack: %v", err)
	}
}

// TestTagTrack_FfmpegFailure verifies that a non-zero ffmpeg exit code is
// propagated as an error and does not corrupt or remove the original file.
func TestTagTrack_FfmpegFailure(t *testing.T) {
	tgr := newTestTagger("FAKE_FFMPEG_FAIL=1")
	audioPath := makeAudioFile(t)

	if err := tgr.TagTrack(audioPath, minimalTrack()); err == nil {
		t.Fatal("expected error from failed ffmpeg, got nil")
	}

	// Original file must still be present.
	if _, err := os.Stat(audioPath); err != nil {
		t.Errorf("original audio file was removed after ffmpeg failure: %v", err)
	}

	// Temp output file must be cleaned up.
	ext := filepath.Ext(audioPath)
	tmpOut := strings.TrimSuffix(audioPath, ext) + ".tagged" + ext
	if _, err := os.Stat(tmpOut); !os.IsNotExist(err) {
		t.Errorf("tagged temp file still exists after failure: %s", tmpOut)
	}
}

// TestTagTrack_PassesMetadataArgs verifies that TagTrack passes key -metadata
// title/album/artist flags through to ffmpeg.
func TestTagTrack_PassesMetadataArgs(t *testing.T) {
	argsFile := filepath.Join(t.TempDir(), "ffmpeg-args.txt")
	tgr := newTestTagger("FAKE_FFMPEG_ARGS_FILE=" + argsFile)
	audioPath := makeAudioFile(t)

	if err := tgr.TagTrack(audioPath, minimalTrack()); err != nil {
		t.Fatalf("TagTrack: %v", err)
	}

	data, err := os.ReadFile(argsFile)
	if err != nil {
		t.Fatalf("read args file: %v", err)
	}
	argsStr := string(data)

	for _, want := range []string{"title=Test Track", "album=Test Album", "artist=Test Artist"} {
		if !strings.Contains(argsStr, want) {
			t.Errorf("expected ffmpeg arg %q not found in:\n%s", want, argsStr)
		}
	}
}

// ---- TagEpisode --------------------------------------------------------

// TestTagEpisode_HappyPath verifies that TagEpisode completes without error
// and leaves the audio file intact.
func TestTagEpisode_HappyPath(t *testing.T) {
	tgr := newTestTagger()
	audioPath := makeAudioFile(t)

	if err := tgr.TagEpisode(audioPath, minimalEpisode()); err != nil {
		t.Fatalf("TagEpisode: %v", err)
	}

	if _, err := os.Stat(audioPath); err != nil {
		t.Errorf("audio file no longer exists after TagEpisode: %v", err)
	}
}

// TestTagEpisode_FfmpegFailure verifies that a non-zero ffmpeg exit is
// propagated correctly for episodes.
func TestTagEpisode_FfmpegFailure(t *testing.T) {
	tgr := newTestTagger("FAKE_FFMPEG_FAIL=1")
	audioPath := makeAudioFile(t)

	if err := tgr.TagEpisode(audioPath, minimalEpisode()); err == nil {
		t.Fatal("expected error from failed ffmpeg, got nil")
	}
}

// TestTagEpisode_PassesMetadataArgs verifies that TagEpisode includes episode-
// specific -metadata flags in the ffmpeg invocation.
func TestTagEpisode_PassesMetadataArgs(t *testing.T) {
	argsFile := filepath.Join(t.TempDir(), "ffmpeg-args.txt")
	tgr := newTestTagger("FAKE_FFMPEG_ARGS_FILE=" + argsFile)
	audioPath := makeAudioFile(t)

	if err := tgr.TagEpisode(audioPath, minimalEpisode()); err != nil {
		t.Fatalf("TagEpisode: %v", err)
	}

	data, err := os.ReadFile(argsFile)
	if err != nil {
		t.Fatalf("read args file: %v", err)
	}
	argsStr := string(data)

	for _, want := range []string{"title=Test Episode", "album=Test Show", "artist=Test Show"} {
		if !strings.Contains(argsStr, want) {
			t.Errorf("expected ffmpeg arg %q not found in:\n%s", want, argsStr)
		}
	}
}

// ---- buildArgs ---------------------------------------------------------

// TestBuildArgs_OGGNoCoverDoesNotUsePicStream verifies the OGG-specific branch
// does not add -map or -c:v copy entries when there is no cover.
func TestBuildArgs_OGGNoCoverDoesNotUsePicStream(t *testing.T) {
	tgr := New("ffmpeg", false)
	args := tgr.buildArgs("audio.ogg", "audio.tagged.ogg", map[string]string{"title": "T"}, "")
	for _, a := range args {
		if a == "-map" || a == "-c:v" {
			t.Errorf("OGG without cover should not include %q arg; got args: %v", a, args)
		}
	}
}

// TestBuildArgs_MP3HasId3v2 verifies the MP3 branch sets -id3v2_version 3.
func TestBuildArgs_MP3HasId3v2(t *testing.T) {
	tgr := New("ffmpeg", false)
	args := tgr.buildArgs("audio.mp3", "audio.tagged.mp3", map[string]string{}, "/tmp/cover.jpg")
	found := false
	for i, a := range args {
		if a == "-id3v2_version" && i+1 < len(args) && args[i+1] == "3" {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("MP3 with cover should include -id3v2_version 3; args: %v", args)
	}
}
