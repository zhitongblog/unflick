#!/usr/bin/env bash
#
# Cut a release: check, bump the version in the three places that carry it,
# commit, tag, push. GitHub Actions builds Windows and Linux from the tag and
# opens a draft release; ./scripts/build-mac.sh adds the macOS .dmg.
#
# Usage: ./scripts/release.sh 0.13.0 [--skip-tests]
#
# The tests run here rather than only in CI because the version number is the
# one thing that cannot be taken back once it is pushed — a tag that has to be
# deleted has usually already been fetched by someone.

set -euo pipefail

cd "$(dirname "$0")/.."

VERSION="${1:-}"
SKIP_TESTS=0
for arg in "${@:2}"; do
  case "$arg" in
    --skip-tests) SKIP_TESTS=1 ;;
    *) echo "unknown argument: $arg" >&2; exit 1 ;;
  esac
done

if [ -z "$VERSION" ]; then
  echo "Usage: $0 <version> [--skip-tests]" >&2
  echo "Example: $0 0.13.0" >&2
  exit 1
fi
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "ERROR: version must be semver, e.g. 0.13.0" >&2
  exit 1
fi

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [ "$BRANCH" != "master" ]; then
  echo "ERROR: releases are cut from master, not $BRANCH." >&2
  exit 1
fi
if [ -n "$(git status --porcelain)" ]; then
  echo "ERROR: working tree not clean. Commit or stash first." >&2
  exit 1
fi
if git rev-parse "v$VERSION" >/dev/null 2>&1; then
  echo "ERROR: tag v$VERSION already exists." >&2
  exit 1
fi

git fetch origin master --quiet
if [ "$(git rev-parse HEAD)" != "$(git rev-parse origin/master)" ]; then
  echo "ERROR: local master and origin/master have diverged. Pull or push first." >&2
  exit 1
fi

if [ "$SKIP_TESTS" = "0" ]; then
  echo "==> Frontend: typecheck, tests, build"
  pnpm install --frozen-lockfile
  npx tsc --noEmit
  pnpm test
  pnpm build

  echo "==> Rust: unit and integration tests"
  # The integration tests drive the real binary against a real libmpv, so they
  # need mpv and ffmpeg on PATH — the same two things a user needs.
  (cd src-tauri && cargo test --lib && cargo test --test playback --test understanding -- --test-threads=2)
fi

echo "==> Bumping version to $VERSION"
node -e "
  const fs = require('fs');
  for (const f of ['package.json', 'src-tauri/tauri.conf.json']) {
    const j = JSON.parse(fs.readFileSync(f, 'utf8'));
    j.version = '$VERSION';
    fs.writeFileSync(f, JSON.stringify(j, null, 2) + '\n');
  }
"
# Only the package's own version line, which is the first one in the file —
# every later 'version = ' belongs to a dependency.
perl -i -pe 'if (!$done && s/^version = "[^"]+"/version = "'"$VERSION"'"/) { $done = 1 }' src-tauri/Cargo.toml

# Cargo.lock carries the version too, and a stale lock makes the first CI
# build re-resolve and fail the --frozen check.
(cd src-tauri && cargo metadata --format-version 1 >/dev/null)

# The same guard build-both.sh applies, applied before the tag rather than
# after a full release build has already run.
PKG="$(node -p "require('./package.json').version")"
CARGO="$(grep -m1 '^version = ' src-tauri/Cargo.toml | cut -d'"' -f2)"
CONF="$(node -p "require('./src-tauri/tauri.conf.json').version")"
if [ "$PKG" != "$VERSION" ] || [ "$CARGO" != "$VERSION" ] || [ "$CONF" != "$VERSION" ]; then
  echo "ERROR: version bump did not take: package.json=$PKG Cargo.toml=$CARGO tauri.conf.json=$CONF" >&2
  exit 1
fi

git add package.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/tauri.conf.json
git commit -m "chore: bump version to $VERSION"
git tag "v$VERSION"

echo "==> Pushing master and v$VERSION"
git push origin master
git push origin "v$VERSION"

cat <<NEXT

==> Pushed. What happens now:

  1. CI builds Windows (standard + AI) and Linux (deb/rpm/AppImage) and
     opens a DRAFT release:
       https://github.com/zhitongblog/unflick/actions

  2. Build and attach the macOS half from this machine:
       ./scripts/build-mac.sh --upload

  3. Write the release notes and publish the draft:
       https://github.com/zhitongblog/unflick/releases

NEXT
