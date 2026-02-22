#!/usr/bin/env bash
set -e

# Bump patch version for all packages
cargo set-version --bump patch

# Commit the version bump
VERSION=$(grep '^version =' Cargo.toml | head -n1 | cut -d '"' -f2)
git add .
git commit -m "chore: bump patch version to v$VERSION"

# Create and push tag
TAG="v$VERSION"
git tag $TAG
git push origin main
git push origin $TAG

echo "Bumped to $VERSION and tagged $TAG"
