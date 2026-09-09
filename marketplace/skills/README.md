# unflick Claude Skills

Seven skills that wrap common unflick workflows. Each is a folder with a
`SKILL.md` (YAML frontmatter + instructions). They assume the **unflick MCP
server** is connected (preferred) or the `unflick` CLI is on PATH.

| Skill | What it does |
|---|---|
| [`unflick-play`](./unflick-play/) | Open & control playback (files, discs, 30+ streaming sites, casting) |
| [`unflick-clip-gif`](./unflick-clip-gif/) | Cut clips / make GIFs |
| [`unflick-subtitles`](./unflick-subtitles/) | Generate (local whisper), translate, and find subtitles online |
| [`unflick-navigate`](./unflick-navigate/) | Seek by what was said, chapters, bookmarks |
| [`unflick-library`](./unflick-library/) | Scan & search a media library |
| [`unflick-screenshot`](./unflick-screenshot/) | Capture frames, look at them, tune the picture |
| [`unflick-sponsorblock`](./unflick-sponsorblock/) | Inspect YouTube SponsorBlock segments |

## Install (any of)

**Claude Code plugin (recommended — also wires up the MCP server):**
```
/plugin marketplace add zhitongblog/unflick
/plugin install unflick
```

**Manual (Claude Code / Claude Desktop skills):**
```bash
# user-level
cp -r unflick-* ~/.claude/skills/
# or project-level
cp -r unflick-* .claude/skills/
```

## Frontmatter convention

```yaml
---
name: unflick-play          # lowercase-hyphen, matches the folder name
description: >-             # when to use it — this is what the model matches on
  Open and control video playback…
license: MIT
---
```

Keep `description` action- and trigger-oriented ("Use when the user asks to…")
so the model reliably picks the right skill.
