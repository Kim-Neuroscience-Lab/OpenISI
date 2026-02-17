#!/usr/bin/env bash
set -euo pipefail

# Bump version in all source-of-truth files, commit, and tag.
#
# Usage: ./scripts/bump-version.sh 0.2.0

if [[ $# -ne 1 ]]; then
    echo "Usage: $0 <version>"
    echo "Example: $0 0.2.0"
    exit 1
fi

VERSION="$1"

# Validate semver format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: Version must be in semver format (e.g., 0.2.0)"
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "Bumping version to $VERSION..."

# 1. src/autoload/version.gd
VERSION_GD="$PROJECT_DIR/src/autoload/version.gd"
sed -i '' "s/^const CURRENT := \".*\"/const CURRENT := \"$VERSION\"/" "$VERSION_GD"
echo "  Updated $VERSION_GD"

# 2. pyproject.toml
PYPROJECT="$PROJECT_DIR/pyproject.toml"
sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" "$PYPROJECT"
echo "  Updated $PYPROJECT"

# 3. extension/Cargo.toml
CARGO="$PROJECT_DIR/extension/Cargo.toml"
sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" "$CARGO"
echo "  Updated $CARGO"

# 4. export_presets.cfg (macOS short_version and version)
PRESETS="$PROJECT_DIR/export_presets.cfg"
sed -i '' "s|application/short_version=\".*\"|application/short_version=\"$VERSION\"|" "$PRESETS"
sed -i '' "s|application/version=\".*\"|application/version=\"$VERSION\"|" "$PRESETS"
# Windows file_version and product_version (x.y.z.0 format)
sed -i '' "s|application/file_version=\".*\"|application/file_version=\"$VERSION.0\"|" "$PRESETS"
sed -i '' "s|application/product_version=\".*\"|application/product_version=\"$VERSION.0\"|" "$PRESETS"
echo "  Updated $PRESETS"

# Commit and tag
cd "$PROJECT_DIR"
git add "$VERSION_GD" "$PYPROJECT" "$CARGO" "$PRESETS"
git commit -m "Release v$VERSION"
git tag "v$VERSION"

echo ""
echo "Version bumped to $VERSION"
echo "  Commit created and tagged v$VERSION"
echo ""
echo "Push with:"
echo "  git push && git push --tags"
