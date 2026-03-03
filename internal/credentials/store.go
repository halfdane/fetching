// Package credentials manages Spotify OAuth credentials, delegating the
// actual auth flow to fetching-cli.
package credentials

import (
	"encoding/json"
	"fmt"
	"os"
	"sync"
	"time"
)

// Credentials holds the OAuth tokens returned by fetching-cli.
type Credentials struct {
	AccessToken  string `json:"access_token"`
	RefreshToken string `json:"refresh_token"`
	ExpiresAt    int64  `json:"expires_at"`
}

// IsExpired reports whether the access token has expired (with a 60-second buffer).
func (c *Credentials) IsExpired() bool {
	return time.Now().Unix() >= c.ExpiresAt-60
}

// JSON returns the credentials as a JSON byte slice.
func (c *Credentials) JSON() ([]byte, error) {
	return json.Marshal(c)
}

// Store manages credential persistence and automatic refresh.
type Store struct {
	path  string
	mu    sync.Mutex
	creds *Credentials

	// authFn runs fetching-cli auth and returns fresh credentials.
	authFn func() (*Credentials, error)
	// reauthFn runs fetching-cli reauth with existing credentials.
	reauthFn func(creds *Credentials) (*Credentials, error)
}

// NewStore creates a credential store backed by the given file path.
// authFn and reauthFn are callbacks to fetching-cli auth/reauth commands.
func NewStore(path string, authFn func() (*Credentials, error), reauthFn func(*Credentials) (*Credentials, error)) *Store {
	return &Store{
		path:     path,
		authFn:   authFn,
		reauthFn: reauthFn,
	}
}

// Get returns valid credentials, refreshing or authenticating as needed.
func (s *Store) Get() (*Credentials, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	// Try loading from memory first
	if s.creds != nil && !s.creds.IsExpired() {
		return s.creds, nil
	}

	// Try loading from disk
	if s.creds == nil {
		creds, err := s.loadFromDisk()
		if err == nil && !creds.IsExpired() {
			s.creds = creds
			return s.creds, nil
		}
		if err == nil {
			// Loaded but expired — try reauth
			s.creds = creds
		}
	}

	// Reauth if we have a refresh token
	if s.creds != nil && s.creds.RefreshToken != "" {
		fresh, err := s.reauthFn(s.creds)
		if err == nil {
			s.creds = fresh
			if saveErr := s.saveToDisk(); saveErr != nil {
				return s.creds, fmt.Errorf("credentials refreshed but save failed: %w", saveErr)
			}
			return s.creds, nil
		}
		// Reauth failed — fall through to full auth
	}

	// Full auth flow
	fresh, err := s.authFn()
	if err != nil {
		return nil, fmt.Errorf("authenticate: %w", err)
	}
	s.creds = fresh
	if saveErr := s.saveToDisk(); saveErr != nil {
		return s.creds, fmt.Errorf("authenticated but save failed: %w", saveErr)
	}
	return s.creds, nil
}

func (s *Store) loadFromDisk() (*Credentials, error) {
	data, err := os.ReadFile(s.path)
	if err != nil {
		return nil, err
	}
	var creds Credentials
	if err := json.Unmarshal(data, &creds); err != nil {
		return nil, fmt.Errorf("parse credentials file: %w", err)
	}
	return &creds, nil
}

func (s *Store) saveToDisk() error {
	data, err := json.MarshalIndent(s.creds, "", "  ")
	if err != nil {
		return fmt.Errorf("marshal credentials: %w", err)
	}
	if err := os.WriteFile(s.path, data, 0600); err != nil {
		return fmt.Errorf("write credentials file: %w", err)
	}
	return nil
}
