// Package cover generates composite cover images for playlists.
// It uses only the Go standard library (image, image/draw, image/jpeg).
package cover

import (
	"fmt"
	"image"
	"image/jpeg"
	_ "image/png" // register PNG decoder
	"io"
	"net/http"
	"os"
	"path/filepath"

	"github.com/halfdane/fetching/internal/spotify"
)

const (
	// OutputSize is the width/height of the generated composite cover.
	OutputSize = 600
	// Half is OutputSize / 2.
	Half = OutputSize / 2
)

// SaveAlbumCover fetches the LARGE cover for an album and saves it as cover.jpg
// in the given directory.
func SaveAlbumCover(dir string, covers []spotify.Cover) error {
	c := largeCover(covers)
	if c == nil {
		return nil // no cover available
	}
	return fetchToFile(spotify.CoverURL(c.FileID), coverPath(dir))
}

// SaveShowCover fetches the LARGE cover for a show and saves it as cover.jpg.
func SaveShowCover(dir string, covers []spotify.Cover) error {
	return SaveAlbumCover(dir, covers) // same logic
}

// SavePlaylistCover generates a composite cover from track covers and saves it.
// coverURLs should contain the unique cover URLs for the first few tracks.
func SavePlaylistCover(dir string, coverURLs []string) error {
	if len(coverURLs) == 0 {
		return nil
	}

	if err := os.MkdirAll(dir, 0755); err != nil {
		return fmt.Errorf("create cover directory: %w", err)
	}

	dest := coverPath(dir)

	switch len(coverURLs) {
	case 1:
		return fetchToFile(coverURLs[0], dest)
	case 2:
		return compositeTwo(coverURLs, dest)
	case 3:
		return compositeThree(coverURLs, dest)
	default:
		return compositeFour(coverURLs[:4], dest)
	}
}

// compositeTwo creates a left/right split.
func compositeTwo(urls []string, dest string) error {
	imgs, err := downloadImages(urls)
	if err != nil {
		return err
	}

	canvas := image.NewRGBA(image.Rect(0, 0, OutputSize, OutputSize))

	// Left half
	drawScaled(canvas, image.Rect(0, 0, Half, OutputSize), imgs[0])
	// Right half
	drawScaled(canvas, image.Rect(Half, 0, OutputSize, OutputSize), imgs[1])

	return saveJPEG(canvas, dest)
}

// compositeThree creates an L-shape: left = full height, right top + right bottom.
func compositeThree(urls []string, dest string) error {
	imgs, err := downloadImages(urls)
	if err != nil {
		return err
	}

	canvas := image.NewRGBA(image.Rect(0, 0, OutputSize, OutputSize))

	// Left half (full height)
	drawScaled(canvas, image.Rect(0, 0, Half, OutputSize), imgs[0])
	// Right top
	drawScaled(canvas, image.Rect(Half, 0, OutputSize, Half), imgs[1])
	// Right bottom
	drawScaled(canvas, image.Rect(Half, Half, OutputSize, OutputSize), imgs[2])

	return saveJPEG(canvas, dest)
}

// compositeFour creates a 2x2 grid.
func compositeFour(urls []string, dest string) error {
	imgs, err := downloadImages(urls)
	if err != nil {
		return err
	}

	canvas := image.NewRGBA(image.Rect(0, 0, OutputSize, OutputSize))

	drawScaled(canvas, image.Rect(0, 0, Half, Half), imgs[0])
	drawScaled(canvas, image.Rect(Half, 0, OutputSize, Half), imgs[1])
	drawScaled(canvas, image.Rect(0, Half, Half, OutputSize), imgs[2])
	drawScaled(canvas, image.Rect(Half, Half, OutputSize, OutputSize), imgs[3])

	return saveJPEG(canvas, dest)
}

// drawScaled draws src into the target rectangle on dst using nearest-neighbor
// scaling. This avoids external dependencies.
func drawScaled(dst *image.RGBA, target image.Rectangle, src image.Image) {
	srcBounds := src.Bounds()
	tw := target.Dx()
	th := target.Dy()
	sw := srcBounds.Dx()
	sh := srcBounds.Dy()

	for y := 0; y < th; y++ {
		for x := 0; x < tw; x++ {
			sx := srcBounds.Min.X + x*sw/tw
			sy := srcBounds.Min.Y + y*sh/th
			dst.Set(target.Min.X+x, target.Min.Y+y, src.At(sx, sy))
		}
	}
}

func saveJPEG(img image.Image, dest string) error {
	f, err := os.Create(dest)
	if err != nil {
		return fmt.Errorf("create cover file: %w", err)
	}
	defer f.Close()

	return jpeg.Encode(f, img, &jpeg.Options{Quality: 90})
}

func coverPath(dir string) string {
	return dir + "/cover.jpg"
}

func largeCover(covers []spotify.Cover) *spotify.Cover {
	for i := range covers {
		if covers[i].Size == "LARGE" {
			return &covers[i]
		}
	}
	// Fallback to DEFAULT, then first
	for i := range covers {
		if covers[i].Size == "DEFAULT" {
			return &covers[i]
		}
	}
	if len(covers) > 0 {
		return &covers[0]
	}
	return nil
}

func downloadImages(urls []string) ([]image.Image, error) {
	imgs := make([]image.Image, len(urls))
	for i, u := range urls {
		img, err := fetchImage(u)
		if err != nil {
			return nil, fmt.Errorf("download cover %d (%s): %w", i, u, err)
		}
		imgs[i] = img
	}
	return imgs, nil
}

func fetchImage(url string) (image.Image, error) {
	resp, err := http.Get(url)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("HTTP %d", resp.StatusCode)
	}

	img, _, err := image.Decode(resp.Body)
	return img, err
}

func fetchToFile(url, dest string) error {
	resp, err := http.Get(url)
	if err != nil {
		return fmt.Errorf("download cover: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("cover download returned status %d", resp.StatusCode)
	}

	if err := os.MkdirAll(filepath.Dir(dest), 0755); err != nil {
		return fmt.Errorf("create cover directory: %w", err)
	}

	f, err := os.Create(dest)
	if err != nil {
		return fmt.Errorf("create cover file: %w", err)
	}
	defer f.Close()

	if _, err := io.Copy(f, resp.Body); err != nil {
		return fmt.Errorf("write cover data: %w", err)
	}
	return nil
}
