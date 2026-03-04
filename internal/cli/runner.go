// Package cli wraps the fetching-cli binary, providing Go functions for
// authentication, metadata fetching, and audio downloading.
//
// The CLI uses a positional argument convention:
//   - 0 args → ensure credentials (interactive auth if needed)
//   - 1 arg  → fetch metadata (JSON on stdout)
//   - 2 args → fetch audio (with -o for direct-to-disk output)
package cli

import (
	"bytes"
	"fmt"
	"os/exec"
)

// Runner executes fetching-cli commands.
type Runner struct {
	// Binary is the path or name of the fetching-cli executable.
	Binary string
}

// NewRunner creates a Runner for the given fetching-cli binary.
// If binary is empty, "fetching-cli" is used (assumes it's on PATH).
func NewRunner(binary string) *Runner {
	if binary == "" {
		binary = "fetching-cli"
	}
	return &Runner{Binary: binary}
}

// EnsureAuth runs fetching-cli with zero arguments, which triggers
// credential loading/refresh/interactive-OAuth as needed. The CLI
// manages its own credential storage.
func (r *Runner) EnsureAuth() error {
	cmd := exec.Command(r.Binary)
	var stderr bytes.Buffer
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return fmt.Errorf("fetching-cli ensure-auth: %w\nstderr: %s", err, stderr.String())
	}
	return nil
}

// FetchMetadata runs `fetching-cli <uri>` (one positional argument) and
// returns the raw metadata JSON from stdout.
func (r *Runner) FetchMetadata(spotifyURI string) ([]byte, error) {
	cmd := exec.Command(r.Binary, spotifyURI)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return nil, fmt.Errorf("fetching-cli fetch metadata %q: %w\nstderr: %s", spotifyURI, err, stderr.String())
	}
	return stdout.Bytes(), nil
}

// FetchAudio runs `fetching-cli <trackURI> <fileID> -o <outputPath>` (two
// positional arguments plus -o flag). The CLI writes audio directly to disk.
func (r *Runner) FetchAudio(trackURI, fileID, outputPath string) error {
	cmd := exec.Command(r.Binary, trackURI, fileID, "-o", outputPath)
	var stderr bytes.Buffer
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return fmt.Errorf("fetching-cli fetch audio %q (file %s): %w\nstderr: %s", trackURI, fileID, err, stderr.String())
	}
	return nil
}
