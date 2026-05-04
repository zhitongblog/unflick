#!/bin/bash
# Build both standard and AI editions of unflick.
#
# Both editions share the same productName ("unflick") so they install to
# the same location and share the same Start menu shortcut. The AI suffix
# is only used in release filenames so the two installers can coexist
# in GitHub releases without overwriting each other.
#
# Standard edition bundles: libmpv, ffmpeg, yt-dlp.
# AI edition adds: whisper-cli + ggml-tiny model + whisper DLLs.
set -e

VERSION="0.9.0"
NSIS_DIR="src-tauri/target/release/bundle/nsis"
MSI_DIR="src-tauri/target/release/bundle/msi"

STANDARD_RESOURCES='[
  "mpv-dev/libmpv-2.dll",
  "ffmpeg/ffmpeg.exe",
  "yt-dlp/yt-dlp.exe"
]'

AI_RESOURCES='[
  "mpv-dev/libmpv-2.dll",
  "ffmpeg/ffmpeg.exe",
  "yt-dlp/yt-dlp.exe",
  "whisper/whisper-cli.exe",
  "whisper/whisper.dll",
  "whisper/ggml.dll",
  "whisper/ggml-base.dll",
  "whisper/ggml-cpu.dll",
  "whisper/ggml-tiny.bin"
]'

apply_resources() {
  local resources="$1"
  node -e "
    const fs = require('fs');
    const conf = JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json', 'utf8'));
    conf.bundle.resources = ${resources};
    conf.productName = 'unflick';
    fs.writeFileSync('src-tauri/tauri.conf.json', JSON.stringify(conf, null, 2));
  "
}

# Restore an EMPTY resources list on exit so the source-controlled
# tauri.conf.json stays clean — the Windows DLLs/EXEs only belong in
# the bundle when build-both.sh is actively building Windows. Leaving
# them in tauri.conf.json after the script exits would cause non-
# Windows builds (Mac DMG, Linux .deb/.rpm/.AppImage) to silently
# bundle 240 MB of useless Windows binaries from src-tauri/{mpv-dev,
# ffmpeg,yt-dlp}/ if those dirs exist on the build machine.
trap 'apply_resources "[]"' EXIT

echo "=== Building Standard Edition ==="
apply_resources "$STANDARD_RESOURCES"
pnpm tauri build
cp "${NSIS_DIR}/unflick_${VERSION}_x64-setup.exe" "${NSIS_DIR}/unflick_${VERSION}_x64-setup-standard.exe"
cp "${MSI_DIR}/unflick_${VERSION}_x64_en-US.msi"  "${MSI_DIR}/unflick_${VERSION}_x64_en-US-standard.msi"
echo "Standard edition built."

echo ""
echo "=== Building AI Edition (with Whisper) ==="
apply_resources "$AI_RESOURCES"
pnpm tauri build
# Rename the AI bundle so it doesn't keep being overwritten by future standard builds.
mv "${NSIS_DIR}/unflick_${VERSION}_x64-setup.exe" "${NSIS_DIR}/unflick-ai_${VERSION}_x64-setup.exe"
mv "${MSI_DIR}/unflick_${VERSION}_x64_en-US.msi"  "${MSI_DIR}/unflick-ai_${VERSION}_x64_en-US.msi"

echo ""
echo "=== Build Complete ==="
echo "Standard: ${NSIS_DIR}/unflick_${VERSION}_x64-setup-standard.exe"
echo "AI:       ${NSIS_DIR}/unflick-ai_${VERSION}_x64-setup.exe"
