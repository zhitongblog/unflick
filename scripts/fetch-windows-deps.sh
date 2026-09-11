#!/usr/bin/env bash
#
# Download the Windows-only binaries that ship inside the Windows installers.
#
# These four directories are deliberately not in git — together they are
# ~450 MB of third-party build output that would dominate the repository and
# go stale on its own schedule. Until now the only copy lived on the
# maintainer's Windows machine, which is why every Windows release had to be
# cut by hand there. This script is what lets CI build the same installers.
#
#   src-tauri/mpv-dev/libmpv-2.dll     the player itself
#   src-tauri/ffmpeg/ffmpeg.exe        thumbnails, clips, cover art, whisper input
#   src-tauri/yt-dlp/yt-dlp.exe        URL playback
#   src-tauri/whisper/                 AI edition only: whisper-cli + ggml + model
#
# Usage:  ./scripts/fetch-windows-deps.sh [--no-whisper]
#
# Re-running is cheap: anything already downloaded is left alone. Delete the
# directory to force a refresh.

set -euo pipefail

cd "$(dirname "$0")/.."

WITH_WHISPER=1
for arg in "$@"; do
  case "$arg" in
    --no-whisper) WITH_WHISPER=0 ;;
    *) echo "unknown argument: $arg" >&2; exit 1 ;;
  esac
done

# 7z is what unpacks the mpv build (it publishes .7z only) and it is present
# on GitHub's windows runners. unzip is not, so 7z handles the .zip files too.
if ! command -v 7z >/dev/null 2>&1; then
  echo "ERROR: 7z not found — needed to unpack the mpv and whisper archives." >&2
  echo "  Windows: it ships with the GitHub runner image; locally, install 7-Zip." >&2
  echo "  macOS:   brew install sevenzip && ln -s \$(which 7zz) /usr/local/bin/7z" >&2
  exit 1
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# GitHub's API is rate-limited per IP for anonymous callers, and CI runners
# share addresses. Pass the runner's token through when there is one.
gh_api() {
  if [ -n "${GITHUB_TOKEN:-}" ]; then
    curl -fsSL -H "Authorization: Bearer $GITHUB_TOKEN" "$1"
  else
    curl -fsSL "$1"
  fi
}

# ---------------------------------------------------------------- libmpv ----
if [ -f src-tauri/mpv-dev/libmpv-2.dll ]; then
  echo "==> libmpv: already present"
else
  echo "==> libmpv: resolving the latest shinchiro build"
  # The release carries four builds; we want plain x86_64, not the i686 or
  # the -v3 variant (which requires AVX2 and would crash on older CPUs).
  MPV_URL="$(gh_api https://api.github.com/repos/shinchiro/mpv-winbuild-cmake/releases/latest \
    | node -e '
      let s = ""; process.stdin.on("data", d => s += d).on("end", () => {
        const a = JSON.parse(s).assets.find(a => /^mpv-dev-x86_64-\d/.test(a.name));
        if (!a) { console.error("no mpv-dev-x86_64 asset in the latest release"); process.exit(1); }
        console.error("    " + a.name);
        console.log(a.browser_download_url);
      });')"
  curl -fL --retry 3 -o "$TMP/mpv-dev.7z" "$MPV_URL"
  mkdir -p src-tauri/mpv-dev
  7z e -y -o"src-tauri/mpv-dev" "$TMP/mpv-dev.7z" libmpv-2.dll >/dev/null
  test -f src-tauri/mpv-dev/libmpv-2.dll
  echo "    src-tauri/mpv-dev/libmpv-2.dll"
fi

# ---------------------------------------------------------------- ffmpeg ----
if [ -f src-tauri/ffmpeg/ffmpeg.exe ]; then
  echo "==> ffmpeg: already present"
else
  echo "==> ffmpeg: downloading the BtbN win64-gpl build"
  curl -fL --retry 3 -o "$TMP/ffmpeg.zip" \
    https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip
  mkdir -p src-tauri/ffmpeg
  # The zip nests everything under one versioned directory, so pull the one
  # file out by name rather than guessing the prefix.
  7z e -y -o"src-tauri/ffmpeg" "$TMP/ffmpeg.zip" "*/bin/ffmpeg.exe" -r >/dev/null
  test -f src-tauri/ffmpeg/ffmpeg.exe
  echo "    src-tauri/ffmpeg/ffmpeg.exe"
fi

# ---------------------------------------------------------------- yt-dlp ----
if [ -f src-tauri/yt-dlp/yt-dlp.exe ]; then
  echo "==> yt-dlp: already present"
else
  echo "==> yt-dlp: downloading the latest release"
  mkdir -p src-tauri/yt-dlp
  curl -fL --retry 3 -o src-tauri/yt-dlp/yt-dlp.exe \
    https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe
  echo "    src-tauri/yt-dlp/yt-dlp.exe"
fi

# --------------------------------------------------------------- whisper ----
if [ "$WITH_WHISPER" = "0" ]; then
  echo "==> whisper: skipped (--no-whisper)"
  exit 0
fi

if [ -f src-tauri/whisper/whisper-cli.exe ] && [ -f src-tauri/whisper/ggml-tiny.bin ]; then
  echo "==> whisper: already present"
else
  echo "==> whisper: resolving a release that carries the Windows binaries"
  # Not `/releases/latest/download/…`: whisper.cpp tags releases that carry no
  # assets at all (v1.9.4 is one), and "latest" points at the newest tag rather
  # than the newest build. That 404 broke a release build once already. Walk
  # back until a release actually has the zip.
  WHISPER_URL="$(gh_api 'https://api.github.com/repos/ggml-org/whisper.cpp/releases?per_page=25' \
    | node -e '
      let s = ""; process.stdin.on("data", d => s += d).on("end", () => {
        for (const r of JSON.parse(s)) {
          const a = (r.assets || []).find(a => a.name === "whisper-bin-x64.zip");
          if (a) { console.error("    " + r.tag_name + " / " + a.name); console.log(a.browser_download_url); return; }
        }
        console.error("no release in the last 25 carries whisper-bin-x64.zip");
        process.exit(1);
      });')"
  curl -fL --retry 3 -o "$TMP/whisper.zip" "$WHISPER_URL"
  mkdir -p src-tauri/whisper
  # whisper.cpp now splits the CPU backend into one DLL per microarchitecture
  # and picks at runtime, so every ggml-cpu-*.dll has to ship — bundling only
  # the ones we can name would leave some machines with no backend at all.
  # The rest of the archive (llama, parakeet, SDL2, the test binaries) is not
  # ours to ship.
  7z e -y -o"src-tauri/whisper" "$TMP/whisper.zip" \
    "Release/whisper-cli.exe" "Release/whisper.dll" "Release/ggml*.dll" >/dev/null
  test -f src-tauri/whisper/whisper-cli.exe

  if [ ! -f src-tauri/whisper/ggml-tiny.bin ]; then
    echo "==> whisper: downloading the tiny model (~75 MB)"
    curl -fL --retry 3 -o src-tauri/whisper/ggml-tiny.bin \
      https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin
  fi
  ls src-tauri/whisper | sed 's/^/    /'
fi
