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

VERSION="0.9.2"
NSIS_DIR="src-tauri/target/release/bundle/nsis"
MSI_DIR="src-tauri/target/release/bundle/msi"

# License files ship with EVERY platform — these are the source-of-truth
# bundle.resources entries that exist in tauri.conf.json by default.
# build-both.sh appends the Windows DLL/EXE list on top while it's
# building Windows installers; the EXIT trap restores to legal-only.
LEGAL_RESOURCES='
  "legal/LICENSE",
  "legal/THIRD-PARTY-LICENSES.md",
  "legal/licenses/LGPL-2.1.txt",
  "legal/licenses/GPL-3.0.txt",
  "legal/licenses/MPL-2.0.txt"'

STANDARD_RESOURCES='[
  "mpv-dev/libmpv-2.dll",
  "ffmpeg/ffmpeg.exe",
  "yt-dlp/yt-dlp.exe",'"$LEGAL_RESOURCES"'
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
  "whisper/ggml-tiny.bin",'"$LEGAL_RESOURCES"'
]'

LEGAL_ONLY_RESOURCES='['"$LEGAL_RESOURCES"'
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

# On exit restore to the legal-only list (license files always ship,
# Windows-specific binaries only ship from this script's active runs).
# Without this, non-Windows builds (Mac DMG, Linux .deb/.rpm/.AppImage)
# would silently bundle 240 MB of Windows binaries from src-tauri/
# {mpv-dev,ffmpeg,yt-dlp}/ if those dirs exist on the build machine.
trap 'apply_resources "$LEGAL_ONLY_RESOURCES"' EXIT

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
