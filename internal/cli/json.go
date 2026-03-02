package cli

import (
	"encoding/json"
	"fmt"
)

// parseJSON is a helper to unmarshal JSON with a descriptive error.
func parseJSON(data []byte, v any) error {
	if err := json.Unmarshal(data, v); err != nil {
		return fmt.Errorf("parse JSON: %w", err)
	}
	return nil
}
