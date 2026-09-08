#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  tools/release.sh <version> [--no-push]

What it does:
  - writes VERSION (the single source of truth; build.rs stamps the binary + the
    Windows version resource + the installer from it — Constitution II)
  - commits VERSION
  - pushes main (unless --no-push)
  - creates tag v<version> and pushes it (unless --no-push)

Version format:
  X.Y.Z            -> stable release  (release.yml: GitHub Release)
  X.Y.Z-beta.N     -> beta release    (release.yml: GitHub *prerelease*)
  The channel a machine follows is compiled into its build (PQ_CHANNEL), not chosen here.

Notes:
  - Requires a clean working tree, on branch main.
  - Tag must not already exist locally or on origin.
  - A stable vX.Y.Z must not be cut until the matching beta has run >= 7 days on >= 3 beta
    instruments with zero Sev-1 telemetry (constitution v1.3.0 — verified via the fleet-status
    view in specs/001-v2-remote-upgrade).
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

# Accept X.Y, X.Y.Z, and X.Y.Z-beta.N
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?(-beta\.[0-9]+)?$ ]]; then
  echo "ERROR: version '$VERSION' is not X.Y[.Z][-beta.N]" >&2
  exit 2
fi

TAG="v${VERSION}"

if [[ -n "$(git status --porcelain)" ]]; then
  echo "ERROR: working tree is not clean. Commit/stash first." >&2
  git status --porcelain >&2
  exit 2
fi

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$BRANCH" != "main" ]]; then
  echo "ERROR: must be on branch 'main' (current: $BRANCH)" >&2
  exit 2
fi

if git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "ERROR: tag already exists locally: $TAG" >&2
  exit 3
fi
if git ls-remote --tags origin "$TAG" | grep -q "$TAG"; then
  echo "ERROR: tag already exists on origin: $TAG" >&2
  exit 3
fi

printf '%s\n' "$VERSION" > VERSION

# v1 (PyInstaller) still consumes these derived files; the v2 Rust build reads VERSION
# directly via build.rs. Non-fatal if the porting script has been removed.
if [[ -f tools/gen_build_versions.py ]]; then
  python tools/gen_build_versions.py || echo "warn: gen_build_versions.py failed (v1 only); continuing"
fi

git add VERSION version.iss version_info.txt 2>/dev/null || git add VERSION
git commit -m "chore(release): ${VERSION}"

if [[ $NO_PUSH -eq 0 ]]; then
  git push origin main
  git tag "$TAG"
  git push origin "$TAG"
else
  echo "--no-push set. Next:"
  echo "  git push origin main && git tag ${TAG} && git push origin ${TAG}"
fi

echo "Release prepared: ${TAG}"
