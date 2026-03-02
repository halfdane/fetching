package main

import (
	"context"
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"

	"github.com/halfdane/fetching/internal/cli"
	"github.com/halfdane/fetching/internal/credentials"
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
  --credentials <path>     Path to credentials JSON file (default: ~/.config/fetching/credentials.json)
  --output <dir>           Output directory for downloaded files (default: ./music)
  --track-template <tmpl>  Path template for tracks (default: "{artist}/{album}/{track_number}-{title}")
  --episode-template <t>   Path template for episodes (default: "{show}/{title}")
  --concurrency <n>        Max parallel downloads (default: 1)
  --verbose                Enable verbose output (default: false)
  <uri> [<uri>...]         Spotify URIs or URLs to download

Serve flags:
  --credentials <path>     Path to credentials JSON file (default: ~/.config/fetching/credentials.json)
  --output <dir>           Output directory for downloaded files (default: ./music)
  --track-template <tmpl>  Path template for tracks (default: "{artist}/{album}/{track_number}-{title}")
  --episode-template <t>   Path template for episodes (default: "{show}/{title}")
  --port <port>            HTTP listen port (default: 8080)
  --concurrency <n>        Max parallel downloads (default: 1)
  --verbose                Enable verbose output (default: false)

Template tokens (tracks):   {artist} {album_artist} {album} {title} {track_number} {disc_number} {year}
Template tokens (episodes): {show} {title} {year} {episode_number}
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

func defaultCredentialsPath() string {
	dir, err := os.UserConfigDir()
	if err != nil {
		dir = filepath.Join(os.Getenv("HOME"), ".config")
	}
	return filepath.Join(dir, "fetching", "credentials.json")
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

func setupDeps(credPath, outputDir, trackTmpl, episodeTmpl string, concurrency int, verbose bool) (*queue.Queue, *worker.Worker, error) {
	runner := cli.NewRunner("")

	credDir := filepath.Dir(credPath)
	if err := os.MkdirAll(credDir, 0700); err != nil {
		return nil, nil, fmt.Errorf("create credentials directory: %w", err)
	}

	credStore := credentials.NewStore(credPath, runner.Auth, runner.Reauth)
	store := storage.NewWithTemplates(outputDir, trackTmpl, episodeTmpl)
	tgr := tagger.New("", verbose)

	q, err := queue.New(dbPath())
	if err != nil {
		return nil, nil, err
	}

	w := worker.New(q, runner, credStore, store, tgr, concurrency)
	return q, w, nil
}

func runBatch(args []string) error {
	fs := flag.NewFlagSet("batch", flag.ExitOnError)
	credPath := fs.String("credentials", defaultCredentialsPath(), "credentials JSON file path")
	outputDir := fs.String("output", "./music", "output directory")
	trackTmpl := fs.String("track-template", "", "path template for tracks (default: \"{artist}/{album}/{track_number}-{title}\")")
	episodeTmpl := fs.String("episode-template", "", "path template for episodes (default: \"{show}/{title}\")")
	concurrency := fs.Int("concurrency", 1, "max parallel downloads")
	verbose := fs.Bool("verbose", false, "enable verbose output")
	fs.Parse(args)

	uris := fs.Args()
	if len(uris) == 0 {
		return fmt.Errorf("no Spotify URIs provided")
	}

	q, w, err := setupDeps(*credPath, *outputDir, *trackTmpl, *episodeTmpl, *concurrency, *verbose)
	if err != nil {
		return err
	}
	defer q.Close()

	if _, err := q.Enqueue(uris...); err != nil {
		return fmt.Errorf("enqueue: %w", err)
	}

	log.Printf("enqueued %d URI(s), processing...", len(uris))

	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer cancel()

	return w.Run(ctx, true)
}

func runServe(args []string) error {
	fs := flag.NewFlagSet("serve", flag.ExitOnError)
	credPath := fs.String("credentials", defaultCredentialsPath(), "credentials JSON file path")
	outputDir := fs.String("output", "./music", "output directory")
	trackTmpl := fs.String("track-template", "", "path template for tracks (default: \"{artist}/{album}/{track_number}-{title}\")")
	episodeTmpl := fs.String("episode-template", "", "path template for episodes (default: \"{show}/{title}\")")
	port := fs.Int("port", 8080, "HTTP listen port")
	concurrency := fs.Int("concurrency", 1, "max parallel downloads")
	verbose := fs.Bool("verbose", false, "enable verbose output")
	fs.Parse(args)

	q, w, err := setupDeps(*credPath, *outputDir, *trackTmpl, *episodeTmpl, *concurrency, *verbose)
	if err != nil {
		return err
	}
	defer q.Close()

	// Start worker in background
	ctx, cancel := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer cancel()

	go func() {
		if err := w.Run(ctx, false); err != nil && ctx.Err() == nil {
			log.Printf("worker error: %v", err)
		}
	}()

	// Start web server
	handler, err := web.New(q)
	if err != nil {
		return fmt.Errorf("init web handler: %w", err)
	}

	mux := http.NewServeMux()
	handler.RegisterRoutes(mux)

	addr := fmt.Sprintf(":%d", *port)
	log.Printf("fetching %s — listening on http://localhost%s", version, addr)

	srv := &http.Server{Addr: addr, Handler: mux}

	go func() {
		<-ctx.Done()
		log.Println("shutting down...")
		srv.Close()
	}()

	if err := srv.ListenAndServe(); err != http.ErrServerClosed {
		return err
	}
	return nil
}
