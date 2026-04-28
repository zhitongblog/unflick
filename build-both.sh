#!/bin/bash
# Build both standard and AI editions of unflick
set -e

echo "=== Building Standard Edition ==="
# Standard build (no whisper bundled)
pnpm tauri build
cp "src-tauri/target/release/bundle/nsis/unflick_0.5.0_x64-setup.exe" "src-tauri/target/release/bundle/nsis/unflick_0.5.0_x64-setup-standard.exe"
cp "src-tauri/target/release/bundle/msi/unflick_0.5.0_x64_en-US.msi" "src-tauri/target/release/bundle/msi/unflick_0.5.0_x64_en-US-standard.msi"
echo "Standard edition built."

echo ""
echo "=== Building AI Edition (with Whisper) ==="
# Add whisper resources to tauri.conf.json temporarily
node -e "
const fs = require('fs');
const conf = JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json', 'utf8'));
conf.bundle.resources = [
  'mpv-dev/libmpv-2.dll',
  'whisper/whisper-cli.exe',
  'whisper/whisper.dll',
  'whisper/ggml.dll',
  'whisper/ggml-base.dll',
  'whisper/ggml-cpu.dll',
  'whisper/ggml-tiny.bin'
];
conf.productName = 'unflick-ai';
fs.writeFileSync('src-tauri/tauri.conf.json', JSON.stringify(conf, null, 2));
"

pnpm tauri build

# Restore original config
node -e "
const fs = require('fs');
const conf = JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json', 'utf8'));
conf.bundle.resources = ['mpv-dev/libmpv-2.dll'];
conf.productName = 'unflick';
fs.writeFileSync('src-tauri/tauri.conf.json', JSON.stringify(conf, null, 2));
"

echo ""
echo "=== Build Complete ==="
echo "Standard: src-tauri/target/release/bundle/nsis/unflick_0.5.0_x64-setup-standard.exe"
echo "AI:       src-tauri/target/release/bundle/nsis/unflick-ai_0.5.0_x64-setup.exe"
