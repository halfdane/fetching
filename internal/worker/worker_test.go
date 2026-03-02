package worker

import (
	"context"
	"errors"
	"testing"
	"time"
)

// TestWithRetry_SucceedsFirstAttempt verifies the happy path.
func TestWithRetry_SucceedsFirstAttempt(t *testing.T) {
	calls := 0
	err := withRetry(context.Background(), "test", nil, func() error {
		calls++
		return nil
	})
	if err != nil {
		t.Errorf("expected nil, got %v", err)
	}
	if calls != 1 {
		t.Errorf("expected 1 call, got %d", calls)
	}
}

// TestWithRetry_ReturnsLastErrorAfterAllAttempts verifies all attempts are tried
// and the last error is wrapped and returned.
func TestWithRetry_ReturnsLastErrorAfterAllAttempts(t *testing.T) {
	origDelays := trackRetryDelays
	trackRetryDelays = []time.Duration{1 * time.Millisecond, 1 * time.Millisecond}
	defer func() { trackRetryDelays = origDelays }()

	calls := 0
	err := withRetry(context.Background(), "test", nil, func() error {
		calls++
		return errors.New("boom")
	})
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	wantCalls := 1 + len(trackRetryDelays)
	if calls != wantCalls {
		t.Errorf("expected %d calls, got %d", wantCalls, calls)
	}
}

// TestWithRetry_ContextCancelledDuringSleep verifies that cancelling the context
// during a retry sleep interrupts the wait promptly and returns ctx.Err().
func TestWithRetry_ContextCancelledDuringSleep(t *testing.T) {
	origDelays := trackRetryDelays
	trackRetryDelays = []time.Duration{5 * time.Second, 10 * time.Second} // long enough to reliably cancel
	defer func() { trackRetryDelays = origDelays }()

	ctx, cancel := context.WithCancel(context.Background())

	calls := 0
	fn := func() error {
		calls++
		return errors.New("transient")
	}

	// Cancel after 50ms, well before the 5s first retry delay fires.
	go func() {
		time.Sleep(50 * time.Millisecond)
		cancel()
	}()

	start := time.Now()
	err := withRetry(ctx, "test", nil, fn)
	elapsed := time.Since(start)

	if !errors.Is(err, context.Canceled) {
		t.Errorf("expected context.Canceled, got %v", err)
	}
	if elapsed > 2*time.Second {
		t.Errorf("withRetry did not exit promptly after cancellation: took %v", elapsed)
	}
	// fn should be called exactly once (initial attempt); retry sleep is cancelled.
	if calls != 1 {
		t.Errorf("expected 1 fn call, got %d (sleep should interrupt before retry)", calls)
	}
}

// TestWithRetry_ContextAlreadyCancelledBeforeCall verifies that a pre-cancelled
// context causes the first attempt to run (fn is called), and if it fails the
// sleep is immediately interrupted.
func TestWithRetry_ContextAlreadyCancelledBeforeCall(t *testing.T) {
	origDelays := trackRetryDelays
	trackRetryDelays = []time.Duration{5 * time.Second}
	defer func() { trackRetryDelays = origDelays }()

	ctx, cancel := context.WithCancel(context.Background())
	cancel() // cancel immediately

	calls := 0
	start := time.Now()
	err := withRetry(ctx, "test", nil, func() error {
		calls++
		return errors.New("fail")
	})
	elapsed := time.Since(start)

	if !errors.Is(err, context.Canceled) {
		t.Errorf("expected context.Canceled, got %v", err)
	}
	if elapsed > 500*time.Millisecond {
		t.Errorf("already-cancelled context did not short-circuit sleep: took %v", elapsed)
	}
}

// TestWithRetry_OnRetryCallbackFired verifies the onRetry callback is called
// before each retry sleep.
func TestWithRetry_OnRetryCallbackFired(t *testing.T) {
	origDelays := trackRetryDelays
	trackRetryDelays = []time.Duration{1 * time.Millisecond, 1 * time.Millisecond}
	defer func() { trackRetryDelays = origDelays }()

	var retryAttempts []int
	err := withRetry(context.Background(), "test", func(attempt, max int, wait time.Duration, lastErr error) {
		retryAttempts = append(retryAttempts, attempt)
	}, func() error {
		return errors.New("fail")
	})

	if err == nil {
		t.Fatal("expected error")
	}
	if len(retryAttempts) != 2 {
		t.Errorf("expected 2 onRetry calls, got %d: %v", len(retryAttempts), retryAttempts)
	}
	if retryAttempts[0] != 1 || retryAttempts[1] != 2 {
		t.Errorf("unexpected retry attempt numbers: %v", retryAttempts)
	}
}
