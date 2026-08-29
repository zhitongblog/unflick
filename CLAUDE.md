# unflick

A modern, beautiful, AI-ready video player for Windows and macOS.

## Project Overview

unflick is a cross-platform video player that provides three interfaces:
- **GUI** — Modern UI for regular users (React + TypeScript + Tailwind)
- **CLI** — Command-line control for power users and scripts
- **MCP Server** — AI agent integration via Model Context Protocol

All three interfaces share a unified Rust core.

## Core Constraint: CLI/MCP First

**Every feature MUST be fully functional via CLI and MCP before it is considered complete.**

This is non-negotiable. The development loop is:
1. Build the Rust core logic
2. Expose it via CLI command + MCP tool
3. Self-test through CLI/MCP (Claude can verify its own work)
4. Only then build the GUI layer on top

A feature without CLI/MCP coverage does NOT ship. The first usable release (v0.1) must be 100% operable through CLI and MCP — GUI is a bonus, not a requirement for v0.1.

### Why
- Claude can self-test every feature during development via CLI and MCP
- Ensures the Rust core is clean and decoupled from any UI
- CLI/MCP users get first-class support, not an afterthought
- Forces good architecture: if it works headless, the GUI is just a skin

### Self-Test Protocol
After implementing any feature, Claude MUST verify it by:
1. Running the CLI command and checking stdout/exit code
2. Calling the MCP tool and validating the JSON response
3. If either fails, fix before moving on — do NOT defer to "test later"
4. **Adding a case to `src-tauri/tests/`** — a verification that lives only
   in a chat transcript is gone the moment the session ends

### Test suite

```bash
pnpm test                  # frontend unit tests (vitest, no DOM needed)

cd src-tauri
cargo test                 # everything: unit + integration
cargo test --lib           # unit only (fast, no media needed)
cargo test --test playback -- --test-threads=2
```

Frontend tests cover the pure logic that has no business being verified by
hand: chord derivation in `lib/keys.ts` (which must agree byte-for-byte
with `core::keybind::normalize`, or a binding stores fine and never fires)
and the gesture/wheel thresholds in `lib/gesture.ts`. They deliberately
avoid a DOM environment — the functions read plain fields, so a test can
pass a plain object.

`tests/playback.rs` and `tests/understanding.rs` drive the **real binary**
against a **real libmpv**, because that's where the bugs live. The two
regressions the suite was written for — a file loading paused at 0:00, and
a finished file reopening on its last frame — both compiled cleanly and
both made the player look broken.

Three environment variables keep a test run from colliding with a player
the developer is using at the time, and from colliding with each other.
**Use all three for manual GUI testing too** — driving the default port
means driving whatever the user is actually watching:

| Variable | Effect |
|---|---|
| `UNFLICK_CONTROL_ADDR` | Control port. Tests bind 29542+ so the real 19542 is never touched |
| `UNFLICK_DATA_DIR` | Library database location. Tests get a throwaway one, so no resume points land in a real watch history |
| `UNFLICK_CONFIG_DIR` | settings.json location — keybindings, mouse bindings, subtitle styling |
| `UNFLICK_LEGACY_DIR` | Where `cleanup` looks for a stale install. Tests point it at a fake one — the real rule (never delete the live caches that share that folder) cannot be exercised against a real machine |
| `UNFLICK_LOG` | Where the startup log is written and where `startup` reads it from. Tests hand it a log they wrote themselves rather than depending on whichever launch happened last |

Fixture media is generated once with the bundled ffmpeg into
`src-tauri/target/test-fixtures/` and reused. `cargo clean` disposes of it.

Integration tests need libmpv and ffmpeg present: vendored on Windows,
`brew install mpv ffmpeg` / `apt install libmpv-dev ffmpeg` elsewhere.
CI (`.github/workflows/ci.yml`) runs all of it on Windows, macOS and Linux.

## Tech Stack

- **Framework**: Tauri 2.x
- **Frontend**: React 18 + TypeScript + Tailwind CSS + Zustand + Framer Motion
- **Backend**: Rust
- **Playback Engine**: libmpv (via Rust FFI)
- **Database**: SQLite (via tauri-plugin-sql)
- **AI (future)**: whisper.cpp for local speech recognition

## Project Structure

```
src/                        # Frontend (React + TS)
  components/
    Player/                 # Player controls (progress bar, volume, etc.)
    Library/                # Media library UI
    Settings/               # Settings page
  hooks/                    # Custom hooks (usePlayer, etc.)
  stores/                   # Zustand state management
  App.tsx
src-tauri/                  # Backend (Rust)
  src/
    main.rs                 # Entry point: detects CLI/MCP/GUI mode
    core/                   # Shared core logic (the single source of truth)
      mod.rs
      player.rs             # Playback control (play, pause, seek, volume...)
      playlist.rs           # Playlist management
      library.rs            # Media library (scan, search, metadata)
      settings.rs           # User settings
    mpv/                    # libmpv FFI bindings
    cli/                    # CLI entry point (clap)
      mod.rs
      commands.rs           # CLI command handlers → call core/
    mcp/                    # MCP server (JSON-RPC over stdio)
      mod.rs
      tools.rs              # MCP tool handlers → call core/
      resources.rs          # MCP resource providers → call core/
    gui/                    # Tauri GUI commands
      commands.rs           # #[tauri::command] handlers → call core/
    db/                     # SQLite operations
  Cargo.toml
  tauri.conf.json
logo/                       # Brand assets
```

### Entry Point Logic (main.rs)

```
unflick                     → launch GUI (Tauri window)
unflick play <file>         → CLI mode (no window, exit after action)
unflick --mcp               → MCP server mode (stdio JSON-RPC, long-running)
```

The binary detects the mode from arguments and routes accordingly. All three modes use the same `core/` module.

## Architecture Principles

- **CLI/MCP first**: every feature works headless before getting a GUI
- GUI, CLI, and MCP share `core/` — no duplicated logic anywhere
- libmpv handles all playback; we do NOT reimplement codec/rendering
- Prefer smart defaults over excessive configuration
- Keep the UI minimal and beautiful; avoid settings bloat
- SQLite for all persistent data (library, play history, settings)
- CLI outputs JSON by default (machine-readable), with `--pretty` for humans
- MCP follows the Model Context Protocol spec strictly

## CLI Design

```bash
# Playback
unflick play <file> [--seek <seconds>] [--volume <0-100>] [--speed <rate>]
# <file> is a path, an http(s) URL, or a path on a mounted share
# (\\server\share\film.mkv, /Volumes/…, /mnt/…). smb:// and nfs:// URLs are
# refused with instructions — no mpv build we ship speaks either protocol.
# The reply carries `loaded`: false means still opening, not on screen yet.
unflick pause
unflick resume
unflick stop
unflick seek <seconds>
unflick volume <0-100>
unflick speed [<rate>] [--relative]     # omit the rate to read it
unflick status                  # JSON: file, position, duration, volume, state

# Playlist
unflick playlist add <file>
unflick playlist list
unflick playlist next
unflick playlist prev
unflick playlist clear
unflick playlist repeat [off|one|all]     # omit value to read
unflick playlist shuffle [on|off]         # omit value to read

# Subtitles
# Audio
unflick audio eq get
unflick audio eq on | off
unflick audio eq band <0-9> <dB>          # 31Hz..16kHz, -12..+12
unflick audio eq curve <10 gains>
unflick audio eq preamp <dB>
unflick audio eq normalize on|off
unflick audio eq preset [name]            # omit to list
unflick audio eq reset
unflick audio pitch [on|off]              # keep pitch when changing speed

unflick subtitle load <file>
unflick subtitle list
unflick subtitle select <id>
unflick subtitle search [query] [--file <path>] [--lang zh-CN,en] [--no-hash]
unflick subtitle download <file_id> [--lang <code>] [--no-load]
unflick subtitle auto [query] [--lang <codes>]     # search + download best match
unflick subtitle delay [<seconds>] [--relative]
unflick subtitle style get
unflick subtitle style set <scale|pos|color|border_size|bold> <value>

# Audio
unflick audio list
unflick audio select <id>
unflick audio delay [<seconds>] [--relative]

# Chapters
unflick chapter list
unflick chapter seek <index>
unflick chapter next
unflick chapter prev
unflick chapter generate [--count <n>]    # derive chapters from the transcript
unflick chapter set '<json>'              # [{"time":0,"title":"Intro"}, ...]
unflick chapter clear

# Transcript (reads the subtitle track the player has open)
unflick transcript get
unflick transcript search <query> [--limit <n>]
unflick transcript seek <query> [--occurrence <n>]

# Picture geometry
unflick video get
unflick video set <aspect|rotate|zoom|panscan|deinterlace> <value>
unflick video reset

# Bookmarks (named positions, kept across sessions)
unflick bookmark add [--name <label>] [--position <s>] [--file <path>]
unflick bookmark list [--file <path>] [--all]
unflick bookmark goto <id>              # seeks, or opens the file if it's another one
unflick bookmark rename <id> <name>     # --clear drops the name
unflick bookmark remove <id>
unflick bookmark clear [--file <path>] [--all]

# Housekeeping
unflick cleanup [--apply]               # files an older install left behind
unflick startup                         # last launch's timeline, phase by phase

# Window and what's playing
unflick window mode [normal|pip|music]  # omit to read; needs the GUI running
unflick nowplaying [--cover]            # title / artist / album / has_video

# Recently played / privacy
unflick recent list [--limit <n>]
unflick recent clear
unflick incognito [on|off]              # omit to read

# Input bindings
unflick keybind list
unflick keybind set <action> <key>
unflick keybind reset [<action>]
unflick mouse list
unflick mouse set <trigger> <action>
unflick mouse reset [<trigger>]

# Frame capture (for multimodal models)
unflick frame capture [--output <path>] [--position <s>] [--max-edge <px>]

# A-B loop / frame stepping
unflick loop a [<seconds>]      # omit to use the current position
unflick loop b [<seconds>]
unflick loop clear
unflick loop status
unflick frame next
unflick frame prev

# Media Library
unflick library scan <dir>
unflick library search <query>
unflick library list [--type <movie|series|video>]

# Utility
unflick screenshot [--output <path>]
unflick clip <start> <end> [--output <path>] [--gif]
unflick info <file>             # JSON: format, duration, resolution, codecs

# Server
unflick --mcp                   # Start MCP server (stdio)
```

## MCP Server Design

### Tools
| Tool | Description | Maps to CLI |
|------|-------------|-------------|
| `play` | Play a file with optional seek/volume | `unflick play` |
| `pause` | Pause playback | `unflick pause` |
| `resume` | Resume playback | `unflick resume` |
| `stop` | Stop playback | `unflick stop` |
| `seek` | Seek to position | `unflick seek` |
| `set_volume` | Set volume level | `unflick volume` |
| `set_speed` | Get or set playback speed (absolute or relative) | `unflick speed` |
| `get_status` | Get playback state | `unflick status` |
| `screenshot` | Capture current frame | `unflick screenshot` |
| `clip` | Extract video segment | `unflick clip` |
| `equalizer_get` / `equalizer_set` | 10-band EQ + normalization | `unflick audio eq` |
| `equalizer_preset` / `equalizer_presets` | Named curves | `unflick audio eq preset` |
| `equalizer_reset` | Clear audio filters | `unflick audio eq reset` |
| `pitch_correction` | Keep pitch when changing speed | `unflick audio pitch` |
| `load_subtitle` | Load subtitle file | `unflick subtitle load` |
| `get_subtitles` | Find + load the best online subtitle | `unflick subtitle auto` |
| `find_subtitles` | Search OpenSubtitles, no download | `unflick subtitle search` |
| `download_subtitle` | Download one search result | `unflick subtitle download` |
| `library_scan` | Scan directory for media | `unflick library scan` |
| `library_search` | Search media library | `unflick library search` |
| `file_info` | Get media file metadata | `unflick info` |
| `subtitle_delay` | Get/set subtitle timing offset | `unflick subtitle delay` |
| `audio_delay` | Get/set audio timing offset | `unflick audio delay` |
| `chapter_list` / `chapter_seek` | Chapter navigation | `unflick chapter …` |
| `ab_loop` | Repeat a section | `unflick loop …` |
| `frame_step` | Step one frame | `unflick frame next` |
| `playlist_repeat` / `playlist_shuffle` | Playback order | `unflick playlist …` |
| `bookmark_add` / `bookmark_list` | Save and read named positions | `unflick bookmark …` |
| `bookmark_goto` | Jump to one, loading its file if needed | `unflick bookmark goto` |
| `bookmark_rename` / `bookmark_remove` / `bookmark_clear` | Manage them | `unflick bookmark …` |
| `window_mode` | Normal / picture-in-picture / music window | `unflick window mode` |
| `now_playing` | Title, artist, album, and whether there is picture | `unflick nowplaying` |
| `cleanup` | Find (and optionally remove) files an older install stranded | `unflick cleanup` |
| `startup` | The last launch's timeline, phase by phase in ms | `unflick startup` |

### Understanding tools

These need the player's own state and can't be replicated by a wrapper
driving unflick from outside:

| Tool | Description | Maps to CLI |
|------|-------------|-------------|
| `search_transcript` | Find a phrase in the open subtitle track, with timestamps | `unflick transcript search` |
| `seek_to_text` | Jump to where a phrase is spoken | `unflick transcript seek` |
| `transcript_get` | Full transcript as timed cues | `unflick transcript get` |
| `generate_chapters` | Derive chapters from transcript pauses | `unflick chapter generate` |
| `set_chapters` | Supply a chapter list the model wrote itself | `unflick chapter set` |
| `describe_frame` | Return the on-screen frame as an image block | `unflick frame capture` |

Generated chapters are real navigation, not just data: they show up in
`chapter_list`, mark the progress bar, and respond to `chapter_seek`. They
are held next to mpv (which can't be given chapters at runtime) and cleared
whenever the file changes.

### Resources
| Resource | Description |
|----------|-------------|
| `unflick://now-playing` | Current playback info (auto-updates) |
| `unflick://library` | Full media library |
| `unflick://playlist` | Current playlist |

## Development Guidelines

### Rust (src-tauri/)
- Use `thiserror` for error types, `anyhow` for application errors
- All core logic lives in `core/` — CLI, MCP, and GUI are thin wrappers
- Keep libmpv interaction isolated in `mpv/` module
- CLI uses `clap` for argument parsing
- MCP uses `serde_json` for JSON-RPC serialization
- All command outputs are `serde::Serialize` structs (shared by CLI JSON and MCP responses)

### Frontend (src/)
- Functional components only, no class components
- State management with Zustand (not Redux)
- Styling with Tailwind CSS utility classes, no CSS modules
- Use `@tauri-apps/api` for backend communication
- Animations with Framer Motion, keep them subtle

### General
- All code in English (comments, variable names, commit messages)
- No unnecessary abstractions — simple and direct
- No feature flags or backward-compatibility shims
- Test at system boundaries, trust internal code

## Key Commands

```bash
# Development
pnpm install              # Install frontend dependencies
pnpm tauri dev            # Run in development mode (GUI)
pnpm tauri build          # Build for production
cargo run -- play test.mp4       # Test CLI during development
cargo run -- --mcp               # Test MCP server during development

# Self-test cycle
cargo run -- play test.mp4 && echo "OK"
cargo run -- status | jq .
cargo run -- screenshot --output /tmp/test.png && ls -la /tmp/test.png
```

## Milestone Plan

### v0.1 — Headless Player (CLI + MCP only, no GUI required)
- Rust core: play, pause, resume, stop, seek, volume, speed, status
- libmpv integration working
- CLI fully functional for all playback commands
- MCP server responding to all playback tools
- Self-tested by Claude through CLI and MCP

### v0.2 — GUI Shell
- Tauri window with video rendering (mpv --wid)
- Basic player controls (play/pause, progress bar, volume)
- Hotkeys (space, arrows, etc.)
- Drag-and-drop file opening

### v0.3 — Playlist & Subtitles
- Playlist management (CLI + MCP + GUI)
- Subtitle loading and switching
- Resume playback (remember position)

### v0.4 — Media Library
- Directory scanning and metadata extraction
- SQLite storage
- Library search (CLI + MCP + GUI)
- Cover art scraping

### v0.5 — Polish & Release
- UI polish, animations, themes
- PiP mode, screenshot, clip extraction
- Installer packaging (Windows .msi, macOS .dmg)

### v0.6 — CLI/MCP-complete + AI subtitles
- Audio track switching (CLI + MCP)
- Playlist jump-by-index (MCP)
- MCP resources: `unflick://now-playing`, `unflick://playlist`, `unflick://library`
- Event-driven `info` probe (replaced 800ms sleep hack with `MPV_EVENT_FILE_LOADED`)
- AI subtitle generation + translation (CLI + MCP, local whisper.cpp + OpenAI API)
- Settings get/set/unset (CLI + MCP, partial-key updates)
- Video filters: brightness/contrast/saturation/gamma/hue (CLI + MCP)
- Every GUI feature now has CLI + MCP coverage — no more first-class/second-class divide

### Future — Advanced AI
- AI scene detection / chapter generation
- Voice-controlled playback ("skip ahead 30 seconds")
- Smart subtitle search ("find where they say 'X'")
- AI-recommended viewing based on library

## Brand

- **Name**: unflick
- **Tagline**: A video player for humans and AI
- **Colors**: Purple `#7C3AED` → Pink `#DB2777` gradient
- **Logo**: `logo/unflick-logo.svg`
