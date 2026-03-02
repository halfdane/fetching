package cover

import (
	"bytes"
	"image"
	"image/color"
	"image/jpeg"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
)

// ---- drawScaled ----

func TestDrawScaledFillsTarget(t *testing.T) {
	// Source: 10x10 solid red
	src := image.NewRGBA(image.Rect(0, 0, 10, 10))
	for y := range 10 {
		for x := range 10 {
			src.Set(x, y, color.RGBA{R: 255, G: 0, B: 0, A: 255})
		}
	}

	dst := image.NewRGBA(image.Rect(0, 0, OutputSize, OutputSize))
	target := image.Rect(0, 0, Half, Half) // top-left quadrant
	drawScaled(dst, target, src)

	// All pixels in target should be red
	for y := range Half {
		for x := range Half {
			r, g, b, _ := dst.At(x, y).RGBA()
			if r>>8 != 255 || g != 0 || b != 0 {
				t.Errorf("pixel (%d,%d) not red: r=%d g=%d b=%d", x, y, r>>8, g>>8, b>>8)
				return
			}
		}
	}
}

func TestDrawScaledDoesNotWriteOutsideTarget(t *testing.T) {
	// Source: 4x4 blue
	src := image.NewRGBA(image.Rect(0, 0, 4, 4))
	for y := range 4 {
		for x := range 4 {
			src.Set(x, y, color.RGBA{B: 255, A: 255})
		}
	}

	dst := image.NewRGBA(image.Rect(0, 0, 20, 20)) // all transparent
	target := image.Rect(5, 5, 10, 10)
	drawScaled(dst, target, src)

	// Pixels outside the target (0,0) should still be transparent
	r, g, b, a := dst.At(0, 0).RGBA()
	if r != 0 || g != 0 || b != 0 || a != 0 {
		t.Errorf("pixel outside target was modified: rgba=(%d,%d,%d,%d)", r>>8, g>>8, b>>8, a>>8)
	}
}

// ---- saveJPEG ----

func TestSaveJPEGCreatesFile(t *testing.T) {
	dir := t.TempDir()
	dest := filepath.Join(dir, "out.jpg")

	img := image.NewRGBA(image.Rect(0, 0, 10, 10))
	if err := saveJPEG(img, dest); err != nil {
		t.Fatalf("saveJPEG error: %v", err)
	}

	info, err := os.Stat(dest)
	if err != nil {
		t.Fatalf("file not created: %v", err)
	}
	if info.Size() == 0 {
		t.Error("file is empty")
	}
}

func TestSaveJPEGProducesValidJPEG(t *testing.T) {
	dir := t.TempDir()
	dest := filepath.Join(dir, "out.jpg")

	img := image.NewRGBA(image.Rect(0, 0, 100, 100))
	if err := saveJPEG(img, dest); err != nil {
		t.Fatal(err)
	}

	data, _ := os.ReadFile(dest)
	if _, err := jpeg.Decode(bytes.NewReader(data)); err != nil {
		t.Errorf("output is not a valid JPEG: %v", err)
	}
}

// ---- composite functions using local test server ----

// makeJPEGServer returns a test server that always responds with a solid-colour JPEG.
func makeJPEGServer(t *testing.T, c color.Color) *httptest.Server {
	t.Helper()
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		img := image.NewRGBA(image.Rect(0, 0, 100, 100))
		for y := range 100 {
			for x := range 100 {
				img.Set(x, y, c)
			}
		}
		w.Header().Set("Content-Type", "image/jpeg")
		jpeg.Encode(w, img, nil)
	}))
}

func TestCompositeTwo(t *testing.T) {
	srv := makeJPEGServer(t, color.RGBA{R: 200, G: 0, B: 0, A: 255})
	defer srv.Close()

	dir := t.TempDir()
	dest := filepath.Join(dir, "cover.jpg")
	err := compositeTwo([]string{srv.URL, srv.URL}, dest)
	if err != nil {
		t.Fatalf("compositeTwo error: %v", err)
	}

	data, _ := os.ReadFile(dest)
	img, err := jpeg.Decode(bytes.NewReader(data))
	if err != nil {
		t.Fatalf("output not valid JPEG: %v", err)
	}
	bounds := img.Bounds()
	if bounds.Dx() != OutputSize || bounds.Dy() != OutputSize {
		t.Errorf("expected %dx%d, got %dx%d", OutputSize, OutputSize, bounds.Dx(), bounds.Dy())
	}
}

func TestCompositeThree(t *testing.T) {
	srv := makeJPEGServer(t, color.RGBA{G: 200, A: 255})
	defer srv.Close()

	dir := t.TempDir()
	dest := filepath.Join(dir, "cover.jpg")
	err := compositeThree([]string{srv.URL, srv.URL, srv.URL}, dest)
	if err != nil {
		t.Fatalf("compositeThree error: %v", err)
	}

	data, _ := os.ReadFile(dest)
	if _, err := jpeg.Decode(bytes.NewReader(data)); err != nil {
		t.Errorf("output not valid JPEG: %v", err)
	}
}

func TestCompositeFour(t *testing.T) {
	srv := makeJPEGServer(t, color.RGBA{B: 200, A: 255})
	defer srv.Close()

	dir := t.TempDir()
	dest := filepath.Join(dir, "cover.jpg")
	err := compositeFour([]string{srv.URL, srv.URL, srv.URL, srv.URL}, dest)
	if err != nil {
		t.Fatalf("compositeFour error: %v", err)
	}

	data, _ := os.ReadFile(dest)
	img, err := jpeg.Decode(bytes.NewReader(data))
	if err != nil {
		t.Fatalf("output not valid JPEG: %v", err)
	}
	if img.Bounds().Dx() != OutputSize {
		t.Errorf("unexpected size: %v", img.Bounds())
	}
}

func TestSavePlaylistCoverRouting(t *testing.T) {
	srv := makeJPEGServer(t, color.RGBA{R: 100, G: 100, B: 100, A: 255})
	defer srv.Close()

	cases := []struct {
		name  string
		urls  []string
	}{
		{"zero urls — no file", []string{}},
		{"one url — direct download", []string{srv.URL}},
		{"two urls — split", []string{srv.URL, srv.URL}},
		{"three urls — L-shape", []string{srv.URL, srv.URL, srv.URL}},
		{"four urls — 2x2 grid", []string{srv.URL, srv.URL, srv.URL, srv.URL}},
		{"five urls — uses first 4", []string{srv.URL, srv.URL, srv.URL, srv.URL, srv.URL}},
	}

	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			dir := t.TempDir()
			err := SavePlaylistCover(dir, c.urls)
			if err != nil {
				t.Fatalf("SavePlaylistCover error: %v", err)
			}
			dest := filepath.Join(dir, "cover.jpg")
			if len(c.urls) == 0 {
				if _, err := os.Stat(dest); !os.IsNotExist(err) {
					t.Error("expected no cover.jpg for zero URLs")
				}
				return
			}
			if _, err := os.Stat(dest); err != nil {
				t.Errorf("cover.jpg not created: %v", err)
			}
		})
	}
}

func TestFetchImageBadStatus(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusNotFound)
	}))
	defer srv.Close()

	_, err := fetchImage(srv.URL)
	if err == nil {
		t.Error("expected error for HTTP 404, got nil")
	}
}
