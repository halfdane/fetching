# fetching

Spotify music fetcher with CLI batch mode and web UI. Wraps [fetching-cli](https://github.com/halfdane/fetching-cli/) for authentication, metadata retrieval, and audio fetching.

## Features

- **Batch mode**: fetch playlists, albums, tracks, shows, or episodes from the command line
- **Web UI**: interactive queue management with live status updates
- **Persistent queue**: SQLite-backed job queue survives restarts
- **Sequential processing**: fetches one track at a time by default (configurable concurrency)
- **Automatic credentials**: handles OAuth auth and token refresh transparently
- **NixOS module**: systemd service with optional nginx reverse proxy

## Requirements

- [fetching-cli](https://github.com/halfdane/fetching-cli/) must be available on `$PATH`
- A Spotify account for authentication

## Installation

### Nix flake

```nix
# flake.nix
{
  inputs.fetching.url = "github:halfdane/fetching";

  # NixOS module
  outputs = { self, fetching, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      modules = [
        fetching.nixosModules.default
        {
          services.fetching = {
            enable = true;
            outputDir = "/media/music";
            port = 8080;
          };
        }
      ];
    };
  };
}
```

### Binary (GitHub release)

Grab a prebuilt binary from the [Releases](https://github.com/halfdane/fetching/releases) page.

### Build from source

```sh
# Enter dev shell (requires Nix + direnv)
direnv allow

# Build
go build -o fetching ./cmd/fetching
```

## Usage

### Batch mode

Fetch one or more Spotify URIs and exit when done:

```sh
fetching batch spotify:album:7FwAtuhhWivxvK4aPgyyUD spotify:track:12l8e8JfVOgX7jQewjyNbU
```

With options:

```sh
fetching batch \
  --credentials ~/.config/fetching/credentials.json \
  --output ~/Music \
  --concurrency 2 \
  spotify:playlist:1e0lJ5eD6xe07D5ooBXhQ9
```

### Web UI

Start the server with a background worker:

```sh
fetching serve --port 8080
```

Then open `http://localhost:8080` to paste Spotify URIs and monitor the fetch queue.

### Credentials

On first run, fetching will launch the Spotify OAuth flow via `fetching-cli auth` and save the credentials to `~/.config/fetching/credentials.json`. Tokens are automatically refreshed when they expire.

## Development

```sh
# Enter dev shell (requires Nix + direnv)
direnv allow

# Build
go build ./cmd/fetching

# Test
go test ./...

# Vet + staticcheck
go vet ./...
staticcheck ./...

# Release
./bump_and_tag.sh        # auto-bump patch
./bump_and_tag.sh 0.2.0  # explicit version
```

## License

MIT
