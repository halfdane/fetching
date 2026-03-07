package worker

import (
	"log/slog"
	"path/filepath"

	"github.com/halfdane/fetching/internal/cover"
	"github.com/halfdane/fetching/internal/playlist"
	"github.com/halfdane/fetching/internal/spotify"
	"github.com/halfdane/fetching/internal/storage"
)

func (w *Worker) generatePlaylistAndCover(meta any, results []fetchResult) {
	if len(results) == 0 {
		return
	}

	switch v := meta.(type) {
	case *spotify.Album:
		w.generateAlbumAssets(v, results)
	case *spotify.Playlist:
		w.generatePlaylistAssets(v, results)
	case *spotify.Show:
		w.generateShowAssets(v, results)
	case *spotify.Track:
		// Single track — no playlist to generate
	case *spotify.Episode:
		// Single episode — no playlist to generate
	}
}

func (w *Worker) generateAlbumAssets(album *spotify.Album, results []fetchResult) {
	if len(results) == 0 {
		return
	}

	// Derive the album dir from where the first track actually landed,
	// so it's always consistent with the path template.
	dir := filepath.Dir(results[0].Path)
	
	// don't store the M3U8 in the track dir: clients know how to read albums from a dir with a cover

	// Cover (LARGE)
	if err := cover.SaveAlbumCover(dir, album.Covers); err != nil {
		slog.Warn("failed to save album cover", "err", err)
	} else {
		slog.Info("saved album cover", "dir", dir)
	}
}

func (w *Worker) generateShowAssets(show *spotify.Show, results []fetchResult) {
	if len(results) == 0 {
		return
	}

	// Derive the show dir from where the first episode actually landed.
	dir := filepath.Dir(results[0].Path)

	entries := resultsToEntries(results)
	m3u8Meta := playlist.Metadata{
		"name":        show.Name,
		"publisher":   show.Publisher,
		"spotify_uri": show.URI,
	}

	dest := dir + "/" + storage.Sanitize(show.Name) + ".m3u8"
	if err := playlist.WriteM3U8(dest, entries, m3u8Meta); err != nil {
		slog.Warn("failed to write show M3U8", "err", err)
	} else {
		slog.Info("wrote show playlist", "path", dest)
	}

	// Shows don't have covers at the Show level in our types,
	// but episodes do — use the first episode's cover if available.
	if results[0].CoverURL != "" {
		if err := cover.SavePlaylistCover(dir, []string{results[0].CoverURL}); err != nil {
			slog.Warn("failed to save show cover", "err", err)
		} else {
			slog.Info("saved show cover", "dir", dir)
		}
	}
}

func (w *Worker) generatePlaylistAssets(pl *spotify.Playlist, results []fetchResult) {
	if len(results) == 0 {
		return
	}

	dir := w.store.PlaylistDir(pl.Name)

	entries := resultsToEntries(results)
	m3u8Meta := playlist.Metadata{
		"name":        pl.Name,
		"description": pl.Description,
		"spotify_uri": pl.URI,
	}

	dest := dir + "/" + storage.Sanitize(pl.Name) + ".m3u8"
	if err := playlist.WriteM3U8(dest, entries, m3u8Meta); err != nil {
		slog.Warn("failed to write playlist M3U8", "err", err)
	} else {
		slog.Info("wrote playlist", "path", dest)
	}

	// Composite cover from unique track covers
	var coverURLs []string
	seen := make(map[string]bool)
	for _, r := range results {
		if r.CoverURL != "" && !seen[r.CoverURL] {
			seen[r.CoverURL] = true
			coverURLs = append(coverURLs, r.CoverURL)
			if len(coverURLs) >= 4 {
				break
			}
		}
	}

	if len(coverURLs) > 0 {
		if err := cover.SavePlaylistCover(dir, coverURLs); err != nil {
			slog.Warn("failed to generate playlist cover", "err", err)
		} else {
			slog.Info("saved playlist cover", "dir", dir)
		}
	}
}

func resultsToEntries(results []fetchResult) []playlist.TrackEntry {
	entries := make([]playlist.TrackEntry, len(results))
	for i, r := range results {
		entries[i] = playlist.TrackEntry{
			Path:        r.Path,
			DurationSec: r.Duration,
			Artist:      r.Artist,
			Title:       r.Title,
		}
	}
	return entries
}
