package main

import (
	"context"
	"flag"
	"fmt"
	"log/slog"
	"net/http"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"syscall"

	"github.com/halfdane/fetching/internal/cli"
	"github.com/halfdane/fetching/internal/logstore"
	"github.com/halfdane/fetching/internal/progress"
	"github.com/halfdane/fetching/internal/queue"
	"github.com/halfdane/fetching/internal/storage"
	"github.com/halfdane/fetching/internal/tagger"
	"github.com/halfdane/fetching/internal/web"
	"github.com/halfdane/fetching/internal/worker"
)

// version is set at build time via -ldflags "-X main.version=vX.Y.Z".
var version = "dev"

const usage = `fetching — Spotify music downloader

Usage:
  fetching <command> [flags]

Commands:
  batch       Enqueue URIs, process them, and exit
  serve       Start the web UI and background worker
  version     Print the version and exit

Batch flags:
  --output <dir>           Output directory for downloaded files (default: ./music)
  --track-template <tmpl>  Path template for tracks (default: "{artist}/{album}/{track_number}-{title}")
  --episode-template <t>   Path template for episodes (default: "{show}/{title}")
  --concurrency <n>        Max parallel downloads (default: 1)
  --fallback-quality       Fall back to lower-quality candidates when all retries fail (default: false)
  --verbose                Enable verbose output (default: false)
  <uri> [<uri>...]         Spotify URIs or URLs to download

Serve flags:
  --output <dir>           Output directory for downloaded files (default: ./music)
  --track-template <tmpl>  Path template for tracks (default: "{artist}/{album}/{track_number}-{title}")
  --episode-template <t>   Path template for episodes (default: "{show}/{title}")
  --port <port>            HTTP listen port (default: 8080)
  --concurrency <n>        Max parallel downloads (default: 1)
  --verbose                Enable verbose output (default: false)

Template tokens (tracks):   {artist} {album_artist} {album} {title} {track_number} {disc_number} {year}
Template tokens (episodes): {show} {title} {year} {episode_number}

Credentials are managed by fetching-cli and stored at ~/.config/fetching-cli/credentials.json.
`

func main() {
	if len(os.Args) < 2 {
		fmt.Fprint(os.Stderr, usage)
		os.Exit(1)
	}

	switch os.Args[1] {
	case "batch":
		if err := runBatch(os.Args[2:]); err != nil {
			fmt.Fprintf(os.Stderr, "error: %v\n", err)
			os.Exit(1)
		}
	case "serve":
		if err := runServe(os.Args[2:]); err != nil {
			fmt.Fprintf(os.Stderr, "error: %v\n", err)
			os.Exit(1)
		}
	case "version", "--version", "-version":
		fmt.Printf("fetching %s\n", version)
	case "-h", "--help", "help":
		fmt.Fprint(os.Stderr, usage)
	default:
		fmt.Fprintf(os.Stderr, "unknown command: %s\n\n", os.Args[1])
		fmt.Fprint(os.Stderr, usage)
		os.Exit(1)
	}
}

func dbPath() string {
	dir, err := os.UserCacheDir()
	if err != nil {
		dir = filepath.Join(os.Getenv("HOME"), ".cache")
	}
	p := filepath.Join(dir, "fetching")
	os.MkdirAll(p, 0755)
	return filepath.Join(p, "fetching.db")
}

func setupDeps(outputDir, trackTmpl, episodeTmpl string, concurrency int, verbose bool) (*cli.Runner, *queue.Queue, *worker.Worker, *progress.Store, error) {
	runner := cli.NewRunner("")
	if _, err := exec.LookPath(runner.Binary); err != nil {
		return nil, nil, nil, nil, fmt.Errorf("%q not found on PATH — install fetching-cli or check your $PATH: %w", runner.Binary, err)
	}
	store := storage.NewWithTemplates(outputDir, trackTmpl, episodeTmpl)
	tgr := tagger.New("", verbose)

	q, err := queue.New(dbPath())
	if err != nil {
		return nil, nil, nil, nil, err
	}
	prog := progress.NewStore()

	w := worker.New(q, runner, store, tgr, prog, concurrency)
	return runner, q, w, prog, nil
}

func runBatch(args []string) error {
	fs := flag.NewFlagSet("batch", flag.ExitOnError)
	outputDir := fs.String("output", "./music", "output directory")
	trackTmpl := fs.String("track-template", "", "path template for tracks (default: \"{artist}/{album}/{track_number}-{title}\")")
	episodeTmpl := fs.String("episode-template", "", "path template for episodes (default: \"{show}/{title}\")")
	concurrency := fs.Int("concurrency", 1, "max parallel downloads")
	fallbackQuality := fs.Bool("fallback-quality", false, "fall back to lower-quality candidates when all retries fail")
	verbose := fs.Bool("verbose", false, "enable verbose output")
	fs.Parse(args)

	uris := fs.Args()
	if len(uris) == 0 {
		return fmt.Errorf("no Spotify URIs provided")
	}

	runner, q, w, _, err := setupDeps(*outputDir, *trackTmpl, *episodeTmpl, *concurrency, *verbose)
	if err != nil {
		return err
	}
	defer q.Close()

	// Ensure credentials are valid before processing any jobs.
	if err := runner.EnsureAuth(); err != nil {
		return fmt.Errorf("authentication: %w", err)
	}

	if err := q.RecoverStuckJobs(); err != nil {
		return fmt.Errorf("recover stuck jobs: %w", err)
	}

	if _, err := q.Enqueue(queue.EnqueueOptions{FallbackQuality: *fallbackQuality}, uris...); err != nil {
		return fmt.Errorf("enqueue: %w", err)
	}

	slog.Info("enqueued URIs, processing", "count", len(uris))

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer cancel()

	return w.Run(ctx, true)
}

func runServe(args []string) error {
	fs := flag.NewFlagSet("serve", flag.ExitOnError)
	outputDir := fs.String("output", "./music", "output directory")
	trackTmpl := fs.String("track-template", "", "path template for tracks (default: \"{artist}/{album}/{track_number}-{title}\")")
	episodeTmpl := fs.String("episode-template", "", "path template for episodes (default: \"{show}/{title}\")")
	port := fs.Int("port", 8080, "HTTP listen port")
	concurrency := fs.Int("concurrency", 1, "max parallel downloads")
	verbose := fs.Bool("verbose", false, "enable verbose output")
	fs.Parse(args)

	runner, q, w, prog, err := setupDeps(*outputDir, *trackTmpl, *episodeTmpl, *concurrency, *verbose)
	if err != nil {
		return err
	}
	defer q.Close()

	// Ensure credentials are valid before starting.
	if err := runner.EnsureAuth(); err != nil {
		return fmt.Errorf("authentication: %w", err)
	}

	if err := q.RecoverStuckJobs(); err != nil {
		return fmt.Errorf("recover stuck jobs: %w", err)
	}

	// Start worker in background
	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer cancel()

	go func() {
		if err := w.Run(ctx, false); err != nil && ctx.Err() == nil {
			slog.Error("worker error", "err", err)
		}
	}()

	// Start web server
	ls := logstore.New()
	slog.SetDefault(slog.New(ls.Handler(os.Stderr)))

	handler, err := web.New(q, prog, ls)
	if err != nil {
		return fmt.Errorf("init web handler: %w", err)
	}

	mux := http.NewServeMux()
	handler.RegisterRoutes(mux)

	addr := fmt.Sprintf(":%d", *port)
	slog.Info("fetching listening", "version", version, "url", "http://localhost"+addr)

	srv := &http.Server{Addr: addr, Handler: mux}

	go func() {
		<-ctx.Done()
		slog.Info("shutting down")
		srv.Close()
	}()

	if err := srv.ListenAndServe(); err != http.ErrServerClosed {
		return err
	}
	return nil
}
