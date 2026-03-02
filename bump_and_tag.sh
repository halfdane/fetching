#!/usr/bin/env bash
set -e

cd "$(git rev-parse --show-toplevel)"

# ── Pre-flight checks ───────────────────────────────────────────────────
echo "Running tests..."
go test ./...

echo "Running vet..."
go vet ./...

echo "Running staticcheck..."
staticcheck ./...

# If tests changed any tracked files, abort so the user can review.
if ! git diff --quiet HEAD; then
  echo "⚠️  Working tree has uncommitted changes." >&2
  echo "   Please review, commit, and re-run." >&2
  exit 1
fi

# ── Bump version ─────────────────────────────────────────────────────────
# Read current version from flake.nix
CURRENT=$(grep 'fetchingVersion = "' flake.nix | sed 's/.*fetchingVersion = "\([^"]*\)".*/\1/')
if [[ -z "$CURRENT" ]]; then
  echo "Could not determine current version from flake.nix" >&2
  exit 1
fi

# Auto-bump patch, or accept an explicit version as first argument
if [[ -n "$1" ]]; then
  NEXT="$1"
else
  IFS='.' read -r major minor patch <<< "$CURRENT"
  NEXT="$major.$minor.$((patch + 1))"
fi

echo "Bumping $CURRENT -> $NEXT"

# Patch flake.nix in place
sed -i "s/fetchingVersion = \"$CURRENT\"/fetchingVersion = \"$NEXT\"/" flake.nix

git add flake.nix
git commit -m "chore: bump version to v$NEXT"

TAG="v$NEXT"
git tag "$TAG"
git push origin main
git push origin "$TAG"

echo "Tagged $TAG"
