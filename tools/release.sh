#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  tools/release.sh <version> [--no-push]

What it does:
  - writes VERSION
  - runs: python tools/gen_build_versions.py
  - commits VERSION + generated files
  - pushes main (unless --no-push)
  - creates tag v<version> and pushes it (unless --no-push)

Notes:
  - Requires a clean working tree.
  - Tag must not already exist.
EOF
}

if [[ ${1:-} == "" || ${1:-} == "-h" || ${1:-} == "--help" ]]; then
  usage
  exit 1
fi

VERSION="$1"
NO_PUSH=0
if [[ ${2:-} == "--no-push" ]]; then
  NO_PUSH=1
fi

TAG="v${VERSION}"

# Ensure clean working tree
if [[ -n "$(git status --porcelain)" ]]; then
  echo "ERROR: Working tree is not clean. Commit/stash your changes first." >&2
  git status --porcelain >&2
  exit 2
fi

# Ensure we are on main
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$BRANCH" != "main" ]]; then
  echo "ERROR: Must be on branch 'main' (current: $BRANCH)" >&2
  exit 2
fi

# Ensure tag does not exist locally or remotely
if git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "ERROR: Tag already exists locally: $TAG" >&2
  exit 3
fi
if git ls-remote --tags origin "$TAG" | grep -q "$TAG"; then
  echo "ERROR: Tag already exists on origin: $TAG" >&2
  exit 3
fi

# Bump version
printf '%s\n' "$VERSION" > VERSION

# Generate derived version files
python tools/gen_build_versions.py

# Sanity: ensure generated files mention the version
if ! grep -q "${VERSION}" version.iss; then
  echo "ERROR: version.iss does not contain expected version ${VERSION}" >&2
  exit 4
fi
if ! grep -q "${VERSION}" version_info.txt; then
  echo "ERROR: version_info.txt does not contain expected version ${VERSION}" >&2
  exit 4
fi

git add VERSION version.iss version_info.txt

git commit -m "chore(release): ${VERSION}"

if [[ $NO_PUSH -eq 0 ]]; then
  git push origin main
  git tag "$TAG"
  git push origin "$TAG"
else
  echo "--no-push set: not pushing branch/tag."
  echo "Next commands would be:"
  echo "  git push origin main"
  echo "  git tag ${TAG}"
  echo "  git push origin ${TAG}"
fi

echo "Release prepared: ${TAG}"
