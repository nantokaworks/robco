#!/usr/bin/env bash
#
# Build the RobCo mdbook specification and publish it to GitHub Pages.
#
# Strategy: build docs/ with mdbook, then publish the generated docs/book/
# directory as a single squashed commit force-pushed to the `gh-pages` branch.
# This never touches the main working tree's git state and needs no extra tools
# beyond mdbook + git.
#
# One-time GitHub setup: repo Settings -> Pages -> Source = "Deploy from a
# branch", Branch = gh-pages / (root).
#
# Overridable via env:
#   PAGES_REMOTE (default: origin)
#   PAGES_BRANCH (default: gh-pages)
set -euo pipefail

REMOTE="${PAGES_REMOTE:-origin}"
BRANCH="${PAGES_BRANCH:-gh-pages}"
SRC_DIR="docs"
OUT_DIR="docs/book"

root="$(git rev-parse --show-toplevel)"
cd "$root"

command -v mdbook >/dev/null 2>&1 || {
  echo "error: mdbook not found on PATH (install: cargo install mdbook or brew install mdbook)" >&2
  exit 1
}

remote_url="$(git remote get-url "$REMOTE")"
rev="$(git rev-parse --short HEAD)"
author_name="$(git config user.name || echo 'robco-pages')"
author_email="$(git config user.email || echo 'robco-pages@users.noreply.github.com')"

echo "==> Building mdbook ($SRC_DIR -> $OUT_DIR)"
rm -rf "$OUT_DIR"
mdbook build "$SRC_DIR"

# Disable Jekyll so GitHub Pages serves assets verbatim (dirs/files that may
# start with an underscore are otherwise ignored).
touch "$OUT_DIR/.nojekyll"

echo "==> Publishing $OUT_DIR to $REMOTE ($BRANCH)"
(
  cd "$OUT_DIR"
  rm -rf .git
  git init -q
  git checkout -q -b "$BRANCH"
  git add -A
  git -c user.name="$author_name" -c user.email="$author_email" \
    commit -q -m "Deploy docs @ $rev"
  git push -q --force "$remote_url" "$BRANCH"
  rm -rf .git
)

echo "==> Done. Pages will publish from '$BRANCH' shortly."
