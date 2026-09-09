# unflick MCP server

> A video player for humans and AI. `unflick --mcp` exposes the full playback
> core to any MCP client over stdio (JSON-RPC 2.0).

- **Protocol**: Model Context Protocol, `2024-11-05`
- **Transport**: stdio
- **Server**: `unflick --mcp` (the npm wrapper, `npx -y unflick-mcp`, is not published yet)
- **Tools**: 94 · **Resources**: 3 · **Prompts**: 0

The MCP server spawns/attaches to the unflick **daemon** (the same core the GUI
and CLI use) and routes every tool call to it, so AI control and the on-screen
window stay in sync.

## Install

```bash
# macOS / Linux
curl -fsSL https://unflick.app/install.sh | bash
# Windows (PowerShell)
irm https://unflick.app/install.ps1 | iex
```

Then add it to your client — see [`client-configs/`](./client-configs/).

## Tools

94 tools. Every one has a 1:1 CLI command — `unflick <thing> <action>` — so a skill can fall back to the shell when no MCP server is connected.

### Playback

| Tool | Args | Description |
|---|---|---|
| `play` | `file` (req), `proxy`, `seek`, `speed`, `volume` | Play a video file or URL |
| `pause` | — | Pause playback |
| `resume` | — | Resume playback after a pause |
| `stop` | — | Stop playback and unload the file |
| `seek` | `seconds` (req) | Seek to a position in seconds |
| `set_volume` | `level` (req) | Set volume level |
| `set_speed` | `rate`, `relative` | Get or set the playback speed of the window the user is watching |
| `set_speed` | `rate`, `relative` | Get or set the playback speed of the window the user is watching |
| `get_status` | — | Get current playback status including state, file, position, duration, volume, and speed |
| `now_playing` | `cover` | What is playing, described the way a person would: title, artist, album, and whether there is any picture (an embedded cover is not video) |
| `file_info` | `file` (req) | Get media file metadata (duration, resolution, codecs) without affecting current playback |
| `save_position` | `path` (req), `position` (req) | Save playback position for a file (for resume playback) |
| `get_position` | `path` (req) | Get saved playback position for a file |
| `frame_step` | — | Step forward exactly one frame |
| `frame_back_step` | — | Step back exactly one frame |
| `ab_loop` | `action` (req), `position` | Control the A-B loop, which repeats a section of the current file |
| `pitch_correction` | `enabled` | Read or set whether changing playback speed keeps the original pitch |

### Chapters

| Tool | Args | Description |
|---|---|---|
| `chapter_list` | — | List the chapters of the currently playing file: index, title, start time, and which one is playing |
| `chapter_seek` | `index` (req) | Jump to a chapter by 0-based index |
| `chapter_next` | — | Jump to the next chapter |
| `chapter_prev` | — | Jump to the previous chapter |
| `generate_chapters` | `count` | Give a file that has no chapters a set of them, derived from the pauses in its transcript |
| `set_chapters` | `chapters` (req) | Set the chapter list for a file that has none, from your own reading of its content |
| `clear_chapters` | — | Remove chapters added by generate_chapters or set_chapters |

### Playlist & recents

| Tool | Args | Description |
|---|---|---|
| `playlist_add` | `file` (req) | Add a file to the playlist |
| `playlist_remove` | `index` (req) | Remove a playlist entry by index |
| `playlist_list` | — | List all playlist entries with index, path, and current track indicator |
| `playlist_next` | — | Advance to and play the next track in the playlist |
| `playlist_prev` | — | Go back to and play the previous track in the playlist |
| `playlist_clear` | — | Clear all entries from the playlist |
| `playlist_play` | `index` (req) | Play a specific playlist entry by index |
| `playlist_repeat` | `mode` | Get or set the playlist repeat mode |
| `playlist_shuffle` | `enabled` | Get or set playlist shuffle |
| `recent_files` | `limit` | Recently played files, newest first, with how many times each was played and when |
| `recent_clear` | — | Forget the play history |

### Subtitles

| Tool | Args | Description |
|---|---|---|
| `load_subtitle` | `file` (req) | Load an external subtitle file |
| `subtitle_list` | — | List all subtitle tracks (embedded and external) |
| `subtitle_select` | `id` (req) | Select a subtitle track by ID (0 to disable subtitles) |
| `subtitle_delay` | `relative`, `seconds` | Get or set the subtitle delay in seconds |
| `subtitle_style_get` | — | Read subtitle appearance: scale, vertical position, colour, border size, bold |
| `subtitle_style_set` | `name` (req), `value` (req) | Set one subtitle appearance property |
| `find_subtitles` | `file`, `hash`, `languages`, `query` | Search OpenSubtitles and return the candidates without downloading anything |
| `download_subtitle` | `file_id` (req), `file`, `language`, `load` | Download one subtitle chosen from find_subtitles and load it into the player |
| `generate_subtitles` | `video` (req), `api_key`, `mode`, `model`, `output_dir`, `whisper` | Generate subtitles for a video using whisper |
| `translate_subtitles` | `api_key` (req), `srt` (req), `target_lang` (req), `output_dir` | Translate an SRT file to another language using OpenAI |
| `get_subtitles` | `file`, `languages`, `load`, `query` | Find and load subtitles for the playing video from OpenSubtitles, in one step |

### Audio

| Tool | Args | Description |
|---|---|---|
| `audio_list` | — | List all audio tracks (embedded) |
| `audio_select` | `id` (req) | Select an audio track by ID (0 to disable audio) |
| `audio_delay` | `relative`, `seconds` | Get or set the audio delay in seconds, for fixing lip-sync |
| `equalizer_get` | — | Read the 10-band equaliser: per-band gains in dB, the band centre frequencies they correspond to, the preamp, whether loudness normalisation is on, and the filter chain mpv is actually running |
| `equalizer_set` | `band`, `bands`, `enabled`, `gain`, `normalize`, `preamp` | Adjust the equaliser |
| `equalizer_preset` | `name` (req) | Apply a named equaliser curve and switch the equaliser on |
| `equalizer_presets` | — | List the available equaliser presets with a description of what each is for and the curve it applies |
| `equalizer_reset` | — | Remove every audio filter: flat curve, no preamp, no normalisation |

### Understanding (what is in the video)

| Tool | Args | Description |
|---|---|---|
| `search_transcript` | `query` (req), `limit` | Search the currently playing file's subtitles for a phrase and return every match with its timestamp |
| `seek_to_text` | `query` (req), `occurrence` | Jump playback to where a phrase is spoken |
| `transcript_get` | — | Return the full transcript of the currently playing file as timed cues, plus where it came from |
| `describe_frame` | `max_edge`, `position` | Return the frame showing right now as an image, so you can see what is on screen |

### Bookmarks

| Tool | Args | Description |
|---|---|---|
| `bookmark_add` | `file`, `name`, `position` | Save a named position in a file, so it can be jumped back to later |
| `bookmark_list` | `all`, `file` | Bookmarks for the file being watched, in timeline order |
| `bookmark_goto` | `id` (req) | Jump to a bookmark by id |
| `bookmark_rename` | `id` (req), `name` | Give a bookmark a name, or drop the one it has by omitting name |
| `bookmark_remove` | `id` (req) | Delete one bookmark by id |
| `bookmark_clear` | `all`, `file` | Delete every bookmark for the file being watched, or for the file named |

### Media library

| Tool | Args | Description |
|---|---|---|
| `library_scan` | `dir` (req) | Scan a directory for video files and add them to the library |
| `library_search` | `query` (req) | Search the media library by title or path |
| `library_list` | — | List all media files in the library |
| `library_remove` | `id` (req) | Remove a media entry from the library by ID |

### Capture & editing

| Tool | Args | Description |
|---|---|---|
| `clip` | `end` (req), `start` (req), `file`, `gif`, `output` | Extract a video clip segment, optionally as GIF |
| `screenshot` | `output` | Take a screenshot of the current video frame |

### Picture

| Tool | Args | Description |
|---|---|---|
| `filter_list` | — | List current video filter values (brightness, contrast, saturation, gamma, hue) |
| `filter_set` | `name` (req), `value` (req) | Set a video filter (brightness | contrast | saturation | gamma | hue) to a value in -100..100 |
| `filter_reset` | — | Reset all video filters to 0 (neutral) |
| `video_transform_get` | — | Read how the picture is fitted to the window: aspect override, rotation, zoom multiplier, pan-scan and deinterlace |
| `video_transform_set` | `name` (req), `value` (req) | Fix a squashed aspect ratio, straighten a video recorded sideways, crop black bars, or deinterlace broadcast footage |
| `video_transform_reset` | — | Restore the file's own geometry — no aspect override, no rotation, no zoom or pan-scan, deinterlace off |

### Window, discs, casting

| Tool | Args | Description |
|---|---|---|
| `window_mode` | `mode` | Get or set the shape of the player window the user is looking at |
| `disc_list` | `path` | Optical drives on this machine and whether each holds a DVD or Blu-ray, plus whether this build can play them at all |
| `cast` | `action`, `file`, `renderer`, `seconds` | Send what is playing to a television on the network over DLNA |

### Input bindings

| Tool | Args | Description |
|---|---|---|
| `keybind_list` | — | List every keyboard shortcut: action id, label, current key, default, and whether the user has changed it |
| `keybind_set` | `action` (req), `key` (req) | Rebind a keyboard shortcut |
| `keybind_reset` | `action` | Restore a shortcut to its default |
| `mouse_list` | — | List mouse bindings: wheel up/down, click, double click, middle click, and the four right-drag gestures, each with the action it runs |
| `mouse_set` | `action` (req), `trigger` (req) | Point a mouse trigger at an action |
| `mouse_reset` | `trigger` | Restore a mouse trigger to its default |

### Streaming / SponsorBlock

| Tool | Args | Description |
|---|---|---|
| `sponsor_segments` | `url` (req) | Fetch SponsorBlock skip segments for a YouTube URL |

### Settings, diagnostics & lifecycle

| Tool | Args | Description |
|---|---|---|
| `settings_path` | — | Get the absolute path of the settings.json file |
| `settings_get` | `key` | Read all settings, or a single key if provided |
| `settings_set` | `key` (req), `value` (req) | Set a single settings key to the given JSON value |
| `settings_unset` | `key` (req) | Remove a single settings key |
| `incognito` | `enabled` | Read or set incognito mode |
| `startup` | — | The last launch's startup timeline, phase by phase, in milliseconds from process start — process init, video pipeline, window shown, the launch file opening, React mounting, the control port |
| `session` | `action` | What the user was last watching and how far in — and, with action "restore", reopen it there |
| `cleanup` | `apply` | Find files an older unflick left behind — on Windows, upgrading from v0.9 moved the install directory and stranded roughly half a gigabyte with no uninstall entry |
| `shutdown` | — | Shut down the unflick daemon |

## Resources

| URI | MIME | Description |
|---|---|---|
| `unflick://now-playing` | application/json | Live: file, position, duration, volume, speed |
| `unflick://playlist` | application/json | Current playlist with current-track indicator |
| `unflick://library` | application/json | Full media library |

## Verify it yourself

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_status","arguments":{}}}' \
  | unflick --mcp
```

Expect `serverInfo: {name: "unflick", version: "0.13.1"}`, a 94-tool list, and a
`get_status` JSON payload.
