// Package cli wraps the fetching-cli binary, providing Go functions for
// authentication, metadata fetching, and audio downloading.
package cli

import (
	"bytes"
	"fmt"
	"io"
	"os/exec"

	"github.com/halfdane/fetching/internal/credentials"
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

// Auth runs `fetching-cli auth` and returns the credentials JSON from stdout.
func (r *Runner) Auth() (*credentials.Credentials, error) {
	cmd := exec.Command(r.Binary, "auth")
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return nil, fmt.Errorf("fetching-cli auth: %w\nstderr: %s", err, stderr.String())
	}

	var creds credentials.Credentials
	if err := parseJSON(stdout.Bytes(), &creds); err != nil {
		return nil, fmt.Errorf("parse auth response: %w", err)
	}
	return &creds, nil
}

// Reauth runs `fetching-cli reauth` with the given credentials piped via stdin.
func (r *Runner) Reauth(creds *credentials.Credentials) (*credentials.Credentials, error) {
	credsJSON, err := creds.JSON()
	if err != nil {
		return nil, fmt.Errorf("marshal credentials for reauth: %w", err)
	}

	cmd := exec.Command(r.Binary, "reauth", "--credentials", "/dev/stdin")
	cmd.Stdin = bytes.NewReader(credsJSON)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return nil, fmt.Errorf("fetching-cli reauth: %w\nstderr: %s", err, stderr.String())
	}

	var fresh credentials.Credentials
	if err := parseJSON(stdout.Bytes(), &fresh); err != nil {
		return nil, fmt.Errorf("parse reauth response: %w", err)
	}
	return &fresh, nil
}

// FetchMetadata runs `fetching-cli fetch <uri>` and returns raw metadata JSON.
func (r *Runner) FetchMetadata(creds *credentials.Credentials, spotifyURI string) ([]byte, error) {
	credsJSON, err := creds.JSON()
	if err != nil {
		return nil, fmt.Errorf("marshal credentials: %w", err)
	}

	cmd := exec.Command(r.Binary, "fetch", "--credentials", "/dev/stdin", spotifyURI)
	cmd.Stdin = bytes.NewReader(credsJSON)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return nil, fmt.Errorf("fetching-cli fetch metadata %q: %w\nstderr: %s", spotifyURI, err, stderr.String())
	}
	return stdout.Bytes(), nil
}

// FetchAudio runs `fetching-cli fetch --track-uri <trackURI> <fileID>` and
// streams the raw audio bytes to the provided writer.
func (r *Runner) FetchAudio(creds *credentials.Credentials, trackURI, fileID string, w io.Writer) error {
	credsJSON, err := creds.JSON()
	if err != nil {
		return fmt.Errorf("marshal credentials: %w", err)
	}

	cmd := exec.Command(r.Binary, "fetch",
		"--credentials", "/dev/stdin",
		"--track-uri", trackURI,
		fileID,
	)
	cmd.Stdin = bytes.NewReader(credsJSON)
	cmd.Stdout = w
	var stderr bytes.Buffer
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return fmt.Errorf("fetching-cli fetch audio %q (file %s): %w\nstderr: %s", trackURI, fileID, err, stderr.String())
	}
	return nil
}
