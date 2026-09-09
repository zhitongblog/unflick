# Listing assets checklist

Most directories show an icon + a few screenshots. Gather these once and reuse
them across every submission.

## Required

- [ ] **Icon** — square PNG, ≥256×256 (512×512 ideal). Source: `logo/unflick-logo.svg`
      → export to PNG. Host at a stable URL (e.g. `https://unflick.app/logo.png`,
      referenced by `mcp/manifest.json` and `mcp/glama.json`).
- [ ] **One-line description** (≤120 chars) — already in `manifest.json`.
- [ ] **Long description / README** — `mcp/README.md`.

## Recommended screenshots (PNG, 16:9, ≥1280px wide)

- [ ] AI agent calling a tool + the unflick window reacting (the headline demo).
- [ ] `tools/list` output or the Claude Desktop MCP panel showing "unflick · 94 tools".
- [ ] A generated/translated subtitle track playing.
- [ ] The media-library search → play flow.

## Capture tip

unflick can screenshot itself:

```bash
unflick play sample.mp4 && unflick seek 30 && unflick screenshot --output assets/now-playing.png
```

## Links to reuse

- Homepage: https://unflick.app
- Repo: https://github.com/zhitongblog/unflick
- Install: `curl -fsSL https://unflick.app/install.sh | bash`
