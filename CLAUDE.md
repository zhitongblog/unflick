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
unflick pause
unflick resume
unflick stop
unflick seek <seconds>
unflick volume <0-100>
unflick speed <rate>
unflick status                  # JSON: file, position, duration, volume, state

# Playlist
unflick playlist add <file>
unflick playlist list
unflick playlist next
unflick playlist prev
unflick playlist clear

# Subtitles
unflick subtitle load <file>
unflick subtitle list
unflick subtitle select <id>

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
| `set_speed` | Set playback speed | `unflick speed` |
| `get_status` | Get playback state | `unflick status` |
| `screenshot` | Capture current frame | `unflick screenshot` |
| `clip` | Extract video segment | `unflick clip` |
| `load_subtitle` | Load subtitle file | `unflick subtitle load` |
| `library_scan` | Scan directory for media | `unflick library scan` |
| `library_search` | Search media library | `unflick library search` |
| `file_info` | Get media file metadata | `unflick info` |

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
