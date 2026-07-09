#!/usr/bin/env bash
# release.sh - local RobCo release pipeline.
#
# Stages: preflight -> check -> build -> package -> tag -> publish -> verify.
# The pipeline publishes binaries to nantokaworks/robco-releases and updates
# Formula/robco.rb in nantokaworks/homebrew-tap.

set -euo pipefail

DRY_RUN=0
FROM_STAGE="preflight"
TO_STAGE="verify"
NOTES_OVERRIDE=""

usage() {
  cat <<'USAGE'
Usage: scripts/release.sh [--from=<stage>] [--to=<stage>] [--dry-run] [--notes=<text>]

Stages: preflight, check, build, package, tag, publish, verify.
USAGE
}

for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    --from=*) FROM_STAGE="${arg#--from=}" ;;
    --to=*) TO_STAGE="${arg#--to=}" ;;
    --notes=*) NOTES_OVERRIDE="${arg#--notes=}" ;;
    -h|--help) usage; exit 0 ;;
    *) echo "error: unknown argument: $arg" >&2; usage >&2; exit 2 ;;
  esac
done

case "$FROM_STAGE" in
  preflight|check|build|package|tag|publish|verify) ;;
  *) echo "error: invalid --from stage: $FROM_STAGE" >&2; exit 2 ;;
esac

case "$TO_STAGE" in
  preflight|check|build|package|tag|publish|verify) ;;
  *) echo "error: invalid --to stage: $TO_STAGE" >&2; exit 2 ;;
esac

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

DIST_DIR="$ROOT/dist"
RELEASES_REPO="nantokaworks/robco-releases"
HOMEBREW_TAP_REPO="nantokaworks/homebrew-tap"
FORMULA_PATH="Formula/robco.rb"

TARGETS=(
  "aarch64-apple-darwin:cargo"
  "x86_64-apple-darwin:cargo"
  "x86_64-unknown-linux-gnu:cross"
  "aarch64-unknown-linux-gnu:cross"
)

log() { printf '[release] %s\n' "$*"; }
die() { printf '[release] error: %s\n' "$*" >&2; exit 1; }

run() {
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '[dry-run] %s\n' "$1"
  else
    bash -c "$1"
  fi
}

stage_active() {
  local stage="$1"
  local order=(preflight check build package tag publish verify)
  local start=-1 end=-1 cur=-1 i
  for i in "${!order[@]}"; do
    [ "${order[$i]}" = "$FROM_STAGE" ] && start=$i
    [ "${order[$i]}" = "$TO_STAGE" ] && end=$i
    [ "${order[$i]}" = "$stage" ] && cur=$i
  done
  [ "$cur" -ge "$start" ] && [ "$cur" -le "$end" ]
}

extract_version() {
  grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/'
}

VERSION="$(extract_version)"
[ -n "$VERSION" ] || die "could not extract version from Cargo.toml"
TAG="v${VERSION}"
log "version=${VERSION} tag=${TAG} root=${ROOT}"
[ "$DRY_RUN" -eq 1 ] && log "DRY-RUN: side-effecting commands will be printed only"

if command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER=sccache
  export SCCACHE_DIR="${SCCACHE_DIR:-$HOME/.cache/sccache}"
  export SCCACHE_CACHE_SIZE="${SCCACHE_CACHE_SIZE:-20G}"
fi

if stage_active preflight; then
  log "stage: preflight"

  [ -z "$(git status --porcelain)" ] || die "working tree is dirty; commit or stash first"

  git fetch origin --tags --prune

  remote_version="$(git show origin/main:Cargo.toml | grep '^version' | head -1 | sed 's/.*"\(.*\)".*/\1/')"
  [ "$VERSION" = "$remote_version" ] || die "local Cargo.toml is ${VERSION}, origin/main is ${remote_version}; merge the bump first"

  if git ls-remote --exit-code --tags origin "refs/tags/${TAG}" >/dev/null 2>&1; then
    die "tag ${TAG} already exists on origin"
  fi

  command -v gh >/dev/null || die "gh CLI not found"
  gh auth status >/dev/null 2>&1 || die "gh not authenticated"
  gh api "repos/${RELEASES_REPO}" --jq .full_name >/dev/null \
    || die "current gh token cannot read ${RELEASES_REPO}"
  gh api "repos/${HOMEBREW_TAP_REPO}" --jq .full_name >/dev/null \
    || die "current gh token cannot read ${HOMEBREW_TAP_REPO}"

  command -v cargo >/dev/null || die "cargo not found"
  command -v rustup >/dev/null || die "rustup not found"

  if ! command -v cross >/dev/null; then
    log "cross not found; installing with cargo"
    run "cargo install cross --git https://github.com/cross-rs/cross"
    if [ "$DRY_RUN" -eq 0 ]; then
      cargo_bin="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin"
      case ":${PATH}:" in
        *":${cargo_bin}:"*) ;;
        *) PATH="${cargo_bin}:${PATH}"; export PATH ;;
      esac
      command -v cross >/dev/null || die "cross install succeeded but cross is not on PATH"
    fi
  fi

  if ! docker info >/dev/null 2>&1; then
    die "Docker daemon not reachable; start Docker Desktop or colima"
  fi

  for entry in "${TARGETS[@]}"; do
    triple="${entry%%:*}"
    if ! rustup target list --installed | grep -qx "$triple"; then
      log "installing rust target ${triple}"
      run "rustup target add ${triple}"
    fi
  done

  log "preflight OK"
fi

if stage_active check; then
  log "stage: check"
  run "cargo fmt --all -- --check"
  run "cargo test --locked"
  run "cargo clippy --all-targets --all-features -- -D warnings"
fi

if stage_active build; then
  log "stage: build"
  for entry in "${TARGETS[@]}"; do
    triple="${entry%%:*}"
    tool="${entry##*:}"
    log "building ${triple} via ${tool}"
    if [ "$tool" = "cross" ]; then
      run "RUSTC_WRAPPER= cross build --release --target ${triple}"
    else
      run "cargo build --release --target ${triple}"
    fi
  done
fi

if stage_active package; then
  log "stage: package"
  run "mkdir -p \"${DIST_DIR}\""
  for entry in "${TARGETS[@]}"; do
    triple="${entry%%:*}"
    bin_dir="${ROOT}/target/${triple}/release"
    archive="${DIST_DIR}/robco-${VERSION}-${triple}.tar.gz"
    if [ "$DRY_RUN" -eq 0 ] && [ ! -x "${bin_dir}/robco" ]; then
      die "binary not found at ${bin_dir}/robco; rerun --from=build"
    fi
    run "tar -C \"${bin_dir}\" -czf \"${archive}\" robco"
  done

  checksums="${DIST_DIR}/robco-v${VERSION}-checksums.txt"
  if [ "$DRY_RUN" -eq 0 ]; then
    (cd "$DIST_DIR" && shasum -a 256 robco-"${VERSION}"-*.tar.gz | tee "$(basename "$checksums")" >/dev/null)
  else
    printf '[dry-run] shasum -a 256 robco-%s-*.tar.gz > %s\n' "$VERSION" "$checksums"
  fi
fi

if stage_active tag; then
  log "stage: tag"
  if git rev-parse "${TAG}" >/dev/null 2>&1; then
    log "local tag ${TAG} already exists; skipping creation"
  else
    run "git tag -a \"${TAG}\" origin/main -m \"Release RobCo ${VERSION}\""
  fi

  if git ls-remote --exit-code --tags origin "refs/tags/${TAG}" >/dev/null 2>&1; then
    log "remote tag ${TAG} already exists; skipping push"
  else
    run "git push origin \"refs/tags/${TAG}\""
  fi
fi

sha_for() {
  shasum -a 256 "$1" | awk '{print $1}'
}

render_formula() {
  local base_url="https://github.com/${RELEASES_REPO}/releases/download/${TAG}"
  local sha_macos_arm sha_macos_x86 sha_linux_arm sha_linux_x86
  sha_macos_arm="$(sha_for "${DIST_DIR}/robco-${VERSION}-aarch64-apple-darwin.tar.gz")"
  sha_macos_x86="$(sha_for "${DIST_DIR}/robco-${VERSION}-x86_64-apple-darwin.tar.gz")"
  sha_linux_arm="$(sha_for "${DIST_DIR}/robco-${VERSION}-aarch64-unknown-linux-gnu.tar.gz")"
  sha_linux_x86="$(sha_for "${DIST_DIR}/robco-${VERSION}-x86_64-unknown-linux-gnu.tar.gz")"
  cat <<RUBY
class Robco < Formula
  desc "Repo-oriented terminal cockpit for supervising AI coding agents"
  homepage "https://github.com/nantokaworks/robco"
  version "${VERSION}"

  livecheck do
    url "https://github.com/${RELEASES_REPO}/releases.atom"
    regex(/v(\d+\.\d+\.\d+)/i)
  end

  on_macos do
    on_arm do
      url "${base_url}/robco-${VERSION}-aarch64-apple-darwin.tar.gz"
      sha256 "${sha_macos_arm}"
    end
    on_intel do
      url "${base_url}/robco-${VERSION}-x86_64-apple-darwin.tar.gz"
      sha256 "${sha_macos_x86}"
    end
  end

  on_linux do
    on_arm do
      url "${base_url}/robco-${VERSION}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "${sha_linux_arm}"
    end
    on_intel do
      url "${base_url}/robco-${VERSION}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "${sha_linux_x86}"
    end
  end

  def install
    bin.install "robco"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/robco --version 2>&1")
  end
end
RUBY
}

if stage_active publish; then
  log "stage: publish"

  notes="${NOTES_OVERRIDE:-RobCo ${VERSION}. Source: https://github.com/nantokaworks/robco}"

  if gh release view "${TAG}" --repo "${RELEASES_REPO}" >/dev/null 2>&1; then
    log "release ${TAG} already exists on ${RELEASES_REPO}; skipping creation"
  else
    assets=("${DIST_DIR}"/robco-"${VERSION}"-*.tar.gz "${DIST_DIR}/robco-v${VERSION}-checksums.txt")
    if [ "$DRY_RUN" -eq 1 ]; then
      printf '[dry-run] gh release create %s --repo %s --title "RobCo %s" --notes "..." %s\n' \
        "$TAG" "$RELEASES_REPO" "$VERSION" "${assets[*]}"
    else
      gh release create "$TAG" \
        --repo "$RELEASES_REPO" \
        --title "RobCo ${VERSION}" \
        --notes "$notes" \
        "${assets[@]}"
    fi
  fi

  if [ "$DRY_RUN" -eq 1 ]; then
    printf '[dry-run] update %s/%s\n' "$HOMEBREW_TAP_REPO" "$FORMULA_PATH"
  else
    formula="$(mktemp)"
    render_formula > "$formula"
    encoded="$(base64 < "$formula" | tr -d '\n')"
    sha="$(gh api "repos/${HOMEBREW_TAP_REPO}/contents/${FORMULA_PATH}" --jq .sha 2>/dev/null || true)"
    message="Update robco formula to ${VERSION}"
    if [ -n "$sha" ]; then
      gh api --method PUT "repos/${HOMEBREW_TAP_REPO}/contents/${FORMULA_PATH}" \
        -f message="$message" -f content="$encoded" -f sha="$sha" >/dev/null
    else
      gh api --method PUT "repos/${HOMEBREW_TAP_REPO}/contents/${FORMULA_PATH}" \
        -f message="$message" -f content="$encoded" >/dev/null
    fi
    rm -f "$formula"
  fi
fi

if stage_active verify; then
  log "stage: verify"
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '[dry-run] gh release view %s --repo %s\n' "$TAG" "$RELEASES_REPO"
    printf '[dry-run] gh api repos/%s/contents/%s (expect version %s)\n' "$HOMEBREW_TAP_REPO" "$FORMULA_PATH" "$VERSION"
  else
    gh release view "$TAG" --repo "$RELEASES_REPO" >/dev/null

    # Read the formula through the authenticated contents API instead of
    # raw.githubusercontent.com: the raw CDN is rate-limited when
    # unauthenticated and lags behind the API write from the publish stage.
    formula_version=""
    for attempt in 1 2 3; do
      formula_version="$(gh api "repos/${HOMEBREW_TAP_REPO}/contents/${FORMULA_PATH}" --jq .content 2>/dev/null \
        | base64 -d | sed -n 's/^ *version "\(.*\)"$/\1/p' | head -1)"
      [ "$formula_version" = "$VERSION" ] && break
      if [ "$attempt" -lt 3 ]; then
        log "formula version check got '${formula_version:-none}' (attempt ${attempt}/3); retrying in 5s"
        sleep 5
      fi
    done
    [ "$formula_version" = "$VERSION" ] \
      || die "formula version is '${formula_version:-none}', expected ${VERSION}"
    log "verify OK"
  fi
fi

log "release complete"
