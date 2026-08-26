# unflick

> A free, ad-free video player — for humans and AI.

**[unflick.app](https://unflick.app)** · MIT · Cross-platform · CLI + MCP first

unflick plays your video files and your YouTube / Bilibili / Twitch / Vimeo / 26 other streaming-site URLs, in a beautiful native window. Auto-skips YouTube sponsor segments via SponsorBlock. Finds subtitles on OpenSubtitles, or generates them locally with whisper.cpp when none exist.

What makes it different: **MCP is a built-in control surface, not a third-party wrapper.** Other players can be scripted from the outside; in unflick the CLI, the MCP server and the window share one player instance. Ask Claude to skip to the next chapter and the video *you are watching* skips — same mpv, same playback, no second hidden process. Every feature ships headless before it gets a button.

## Install

**One-line install:**

```bash
# macOS / Linux
curl -fsSL https://unflick.app/install.sh | bash

# Windows (PowerShell)
irm https://unflick.app/install.ps1 | iex
```

**Or pick an installer for your platform** from the [latest release](https://github.com/zhitongblog/unflick/releases/latest):

| Platform | File | Size |
|---|---|---|
| Windows 10/11 (x64) | `unflick_<ver>_x64-setup-standard.exe` (or `.msi`) | ~80 MB |
| Windows 10/11 (x64), AI edition with bundled Whisper | `unflick-ai_<ver>_x64-setup.exe` | ~150 MB |
| macOS 11+ (Apple Silicon + Intel, signed + notarized) | `unflick_<ver>_universal.dmg` | ~15 MB |
| Linux x86_64 (Debian / Ubuntu) | `unflick_<ver>_amd64.deb` | ~10 MB |
| Linux x86_64 (Fedora / RHEL / openSUSE) | `unflick-<ver>-1.x86_64.rpm` | ~10 MB |
| Linux x86_64 (distro-agnostic) | `unflick_<ver>_amd64.AppImage` | ~80 MB |

## Three interfaces, one core

unflick exposes the same playback engine through three surfaces:

- **GUI** — modern player window with libmpv-quality playback, keyboard shortcuts, drag-and-drop, picture-in-picture, true fullscreen, chapters, bookmarks, A-B loop, frame stepping, subtitle timing and styling.
- **CLI** — every feature is also a command. `unflick play <file-or-url>`, `unflick chapter next`, `unflick bookmark add --name "the good bit"`, `unflick loop a`, `unflick subtitle auto`, `unflick audio eq preset speech`, `unflick clip 0 5`, `unflick library scan`. Output is JSON; pipe it to `jq` and automate.
- **MCP server** — `unflick --mcp` starts a Model Context Protocol server over stdio. Add it to Claude Desktop / Cursor / Codex CLI's MCP config and your AI agent gets 87 tools (play, seek, chapter_seek, bookmark_goto, ab_loop, subtitle_delay, screenshot, clip, sponsor_segments, get_subtitles, equalizer_preset, generate_subtitles, library_search, …) plus live resources.

All three drive **the same player**. When the window is open it hosts the control port, so `unflick pause` from a terminal — or an agent calling `pause` over MCP — pauses the video on screen rather than some invisible second instance. With no window running, the CLI and MCP fall back to a headless daemon and everything still works.

### What an agent can do that a wrapper can't

Because MCP talks to the player rather than at it, an agent gets tools that need the player's own state:

- **`search_transcript` / `seek_to_text`** — "skip to where she explains the refund policy". Searches whatever subtitles are open: a loaded file, a sidecar `.srt`, an embedded track, or the ones you just generated with whisper.
- **`generate_chapters` / `set_chapters`** — give a file that shipped without chapters a real chapter list. It's not just data back: the chapters appear on the progress bar and respond to `chapter_seek` and PgUp/PgDn.
- **`describe_frame`** — hand the model the frame that's on screen right now, as an image. Same player, so it sees what you see.

## Free, ad-free, local — by charter

unflick will never:

- carry banner / pre-roll / "sponsored" ads
- collect telemetry of any kind
- require an account, login, or cloud connection
- gate features behind a paid tier or trial countdown
- bundle adware, browser hijackers, or "recommended" software at install time

Everything that matters runs on your machine. Library scans, subtitle transcription, AI rewriting — all local. Your files stay on your filesystem.

The SponsorBlock auto-skip for YouTube is the active form of "ad-free" — actively skipping ads in the content unflick plays, not just refusing to show its own.

## License

unflick is released under the **MIT License** — see [LICENSE](LICENSE).

unflick bundles, dynamically loads, or invokes the following third-party software at runtime. Each upstream license is enumerated in [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md), and the canonical license text for the LGPL/GPL/MPL-licensed components ships in the [`licenses/`](licenses/) directory of every installer:

- **libmpv** — LGPL-2.1-or-later — dynamically loaded at runtime ([source](https://github.com/mpv-player/mpv))
- **ffmpeg** (Windows bundle) — GPL-3.0-or-later — invoked as a subprocess only ([source](https://github.com/FFmpeg/FFmpeg))
- **yt-dlp** — Unlicense (public domain) — invoked as a subprocess ([source](https://github.com/yt-dlp/yt-dlp))
- **whisper.cpp** (AI edition) — MIT — invoked as a subprocess ([source](https://github.com/ggml-org/whisper.cpp))
- **Tauri** + **wry** + **mpv-player/mpv** + **glutin** + **objc2** + 590+ Rust crates — all MIT, Apache-2.0, ISC, BSD, MPL-2.0, or Unicode-3.0 ([full breakdown](THIRD-PARTY-LICENSES.md))
- **React 18**, **Zustand**, **Framer Motion**, **Tailwind**, **Vite** — MIT

See [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md) for the complete attribution and compliance notes (subprocess invocation vs. dynamic linking, GPL aggregation, MPL file-level scope, etc.).

## Build from source

```bash
git clone https://github.com/zhitongblog/unflick.git
cd unflick
pnpm install
pnpm tauri build
# Windows: bash build-both.sh   builds both Standard and AI editions
```

Requires Rust stable + Node 18+ + the Tauri 2 platform prerequisites (WebView2 on Windows, WKWebView on macOS, webkit2gtk-4.1 on Linux).

## Acknowledgments

unflick stands on the shoulders of mpv, ffmpeg, yt-dlp, whisper.cpp, Tauri, and a long list of MIT/Apache-licensed Rust and JavaScript libraries. None of this would exist without those projects.

The website at [unflick.app](https://unflick.app) and a sibling project ([SoloMD](https://solomd.app)) — also free, also MIT, also bundling MCP — are by the same author. If you like the design, that's where it comes from.
