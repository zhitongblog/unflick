#!/usr/bin/env bash
#
# Build the macOS release locally: universal .app, Developer ID signature,
# Apple notarization, stapled .dmg.
#
# macOS is missing from .github/workflows/release.yml on purpose. Signing and
# notarization need an Apple developer identity and an App Store Connect key,
# and notarytool on a hosted runner can sit there for hours without saying
# why. So this half of the release runs here, where the credentials already
# are, and uploads into the draft release CI opened.
#
# Usage:
#   ./scripts/build-mac.sh              # build, sign, notarize, staple
#   ./scripts/build-mac.sh --upload     # …and attach the .dmg to the draft
#                                       #  release for the current version
#
# Credentials — put them in .env.local (gitignored) or export them:
#   APPLE_SIGNING_IDENTITY  "Developer ID Application: … (TEAMID)"
#   APPLE_API_KEY + APPLE_API_ISSUER + APPLE_API_KEY_PATH   (preferred)
#   APPLE_ID + APPLE_PASSWORD + APPLE_TEAM_ID               (fallback)
#
# The app dlopens libmpv at runtime rather than linking it, so the .dmg stays
# ~15 MB and works on both architectures from one bundle. Users get mpv from
# Homebrew — install.sh does that for them.

set -euo pipefail

cd "$(dirname "$0")/.."

if [ -f .env.local ]; then
  set -a
  # shellcheck disable=SC1091
  source .env.local
  set +a
fi

: "${APPLE_SIGNING_IDENTITY:?Set APPLE_SIGNING_IDENTITY (see the header of this script)}"

# Tauri notarizes when it finds either credential set in the environment. Say
# so up front instead of discovering it after a fifteen-minute build: an
# unsigned-but-unnotarized .dmg looks fine locally and is refused on every
# machine that did not build it.
if [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ]; then
  : "${APPLE_API_KEY_PATH:?APPLE_API_KEY is set, so APPLE_API_KEY_PATH must point at the .p8}"
  [ -f "$APPLE_API_KEY_PATH" ] || { echo "ERROR: no .p8 at $APPLE_API_KEY_PATH" >&2; exit 1; }
  echo "==> Notarizing with the App Store Connect key ${APPLE_API_KEY}"
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  echo "==> Notarizing with the Apple ID ${APPLE_ID}"
else
  echo "ERROR: no notarization credentials. Set APPLE_API_KEY + APPLE_API_ISSUER +" >&2
  echo "       APPLE_API_KEY_PATH, or APPLE_ID + APPLE_PASSWORD + APPLE_TEAM_ID." >&2
  exit 1
fi

VERSION="$(node -p "require('./package.json').version")"
echo "==> unflick ${VERSION} — universal macOS build"

# Both halves of the universal binary have to exist before Tauri can lipo them.
rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null

pnpm install --frozen-lockfile

APPLE_SIGNING_IDENTITY="$APPLE_SIGNING_IDENTITY" \
  pnpm tauri build --target universal-apple-darwin

BUNDLE="src-tauri/target/universal-apple-darwin/release/bundle"
APP="$BUNDLE/macos/unflick.app"
DMG="$BUNDLE/dmg/unflick_${VERSION}_universal.dmg"

[ -d "$APP" ] || { echo "ERROR: no .app at $APP" >&2; exit 1; }
[ -f "$DMG" ] || { echo "ERROR: no .dmg at $DMG" >&2; exit 1; }

# Three separate questions, each of which has been the thing that was wrong:
# is it universal, is the signature valid, did Apple actually staple a ticket.
echo "==> Verifying"
lipo -archs "$APP/Contents/MacOS/unflick"
codesign --verify --deep --strict --verbose=2 "$APP"
xcrun stapler validate "$APP" || {
  echo "ERROR: the .app has no notarization ticket — Gatekeeper will refuse it." >&2
  exit 1
}
spctl --assess --type execute --verbose=2 "$APP"

ls -lh "$DMG"

if [ "${1:-}" = "--upload" ]; then
  echo "==> Uploading to the draft release v${VERSION}"
  gh release upload "v${VERSION}" "$DMG" --clobber
  gh release view "v${VERSION}" --json assets --jq '.assets[].name'
fi

echo "==> Done: $DMG"
